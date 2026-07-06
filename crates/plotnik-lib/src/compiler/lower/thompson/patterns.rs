//! Pattern compilation for leaf and wrapper patterns.
//!
//! Handles compilation of:
//! - Named nodes: `(identifier)`, `(call_expression ...)`
//! - Anonymous nodes: `"+"`, `_`
//! - References: `(Pattern)` (calls to other definitions)
//! - Field constraints: `name: pattern`
//! - Captured patterns: `@name`, `pattern @name`

use crate::bytecode::{Nav, PredicateOp, SpanKind};
use crate::compiler::analyze::types::TypeShape;
use crate::compiler::ids::DefId;
use crate::compiler::lower::ir::{
    CalleeEntry, EffectIR, InstructionIR, Label, MatchIR, NodeKindConstraint, PredicateIR,
    ReturnAddr,
};
use crate::compiler::parse::ast::{self, Pattern};
use crate::compiler::parse::cst::SyntaxKind;
use crate::compiler::parse::strings::unescape;
use crate::core::NodeFieldId;

use crate::compiler::analyze::types::CaptureKind;

use super::NfaBuilder;
use super::capture::{CaptureEffects, PatternCtx};
use super::navigation::pattern_owns_iteration;
use super::nfa_emit::{BranchTargets, Greediness};
use super::scope::{CaptureExits, CaptureRequest, ScopeCloseEffects, SkipExit, SplitExits};
use super::sequences::SeqItemsCtx;

#[derive(Clone, Copy)]
enum RefLowering {
    ScopedCapture,
    CapturedValue,
    SuppressedCall,
    PlainCall,
}

#[derive(Clone, Copy)]
enum CalleeVariant {
    Full,
    Consuming,
}

/// Whether the post-effect chain consumes the call's pending value.
///
/// A capture on the reference itself puts its consumer (`Set`, or `Push` for an
/// array element) first in the chain — `build_capture_effects` emits no `Node`
/// for the `Ref` mechanism. Anything else first — a `Node`+`Set` pair capturing
/// the matched node, or an enclosing scope's close (`EnumClose` on a tag-only
/// variant) — leaves the reference itself unconsumed, so its output is
/// suppressed.
fn post_consumes_call_value(post: &[EffectIR]) -> bool {
    CaptureEffects::effects_consume_value(post)
}

impl<'a> CaptureRequest<'a> {
    fn build(
        compiler: &mut NfaBuilder<'_>,
        cap: &ast::CapturedPattern,
        inner: &'a Pattern,
        nav: Option<Nav>,
        mechanism: CaptureKind,
        outer_capture: CaptureEffects,
    ) -> Self {
        Self {
            inner,
            nav,
            capture_effects: compiler.build_capture_effects(cap, Some(mechanism)),
            outer_capture,
        }
    }
}

impl NfaBuilder<'_> {
    pub(super) fn compile_node_pattern(
        &mut self,
        node: &ast::NodePattern,
        ctx: PatternCtx,
    ) -> Label {
        let PatternCtx {
            exit,
            nav: nav_override,
            capture,
            value: _,
        } = ctx;
        let entry = self.fresh_label();
        let node_kind = self.resolve_node_kind(node);
        let nav = nav_override.unwrap_or(Nav::Stay);

        let items: Vec<_> = node.items().collect();
        let neg_fields = self.collect_neg_fields(node);
        let predicate = self.compile_predicate(node);

        if items.is_empty() {
            let mut m = MatchIR::epsilon(entry, exit)
                .nav(nav)
                .node_kind(node_kind)
                .neg_fields(neg_fields)
                .prepend_effects(capture.pre)
                .append_effects(capture.post);
            if let Some(p) = predicate {
                m = m.predicate(p);
            }
            return self.emit_match(m);
        }

        let (has_trailing_anchor, trailing_nav) =
            self.anchor_semantics.check_trailing_anchor(&items);

        // Emit Up instruction with appropriate strictness. A trailing anchor only
        // changes this ascent into a lastness check (`Up*`); the body itself
        // compiles like any node body, with `compile_seq_items` keeping the
        // last item's child search resumable so a lastness failure can retry.
        let up_nav = if has_trailing_anchor {
            trailing_nav.unwrap_or(Nav::UpSkipTrivia(1))
        } else {
            Nav::Up(1)
        };

        // Split capture.post: Node effects (and their Set) go on entry so
        // they read the cursor immediately after this node matched. Other
        // effects run after child constraints have completed.
        use crate::bytecode::EffectKind;

        let mut entry_effects = Vec::new();
        let mut exit_effects = Vec::new();
        let mut iter = capture.post.into_iter().peekable();
        while let Some(eff) = iter.next() {
            match eff.kind() {
                // A capture unit `[SpanStart, Node, Set, SpanEnd]` moves to
                // the entry as a whole: the Set must stay adjacent to its
                // pending Node, and the markers must keep hugging the Set.
                EffectKind::SpanStart
                    if iter.peek().is_some_and(|e| e.kind() == EffectKind::Node) =>
                {
                    entry_effects.push(eff);
                    entry_effects.push(iter.next().expect("peeked Node"));
                    if iter.peek().is_some_and(|e| e.kind() == EffectKind::Set) {
                        entry_effects.push(iter.next().expect("peeked Set"));
                    }
                    if iter.peek().is_some_and(|e| e.kind() == EffectKind::SpanEnd) {
                        entry_effects.push(iter.next().expect("peeked SpanEnd"));
                    }
                }
                EffectKind::Node => {
                    entry_effects.push(eff);
                    if iter.peek().is_some_and(|e| e.kind() == EffectKind::Set) {
                        entry_effects.push(iter.next().expect("peeked Set"));
                    }
                }
                _ => exit_effects.push(eff),
            }
        }

        // With items: nav[entry_effects] → items → Up → [exit_effects] → exit
        let final_exit = self.emit_trailing_effects_exit(exit, exit_effects);

        // The skip exit bypasses Up when the whole child list matches
        // zero-width: nothing was consumed, so the cursor never descended and
        // there is no child to ascend from. Anchors lose their carrier on that
        // path — a trailing anchor's "nothing may follow the last match" and a
        // leading anchor's "the first match comes first" both degrade to "this
        // node has no children the anchor's skip policy would reject",
        // asserted by a `Childless*` check. The skip classes nest, so when
        // both anchors demand one, the tighter check alone suffices.
        let trailing_childless = has_trailing_anchor.then(|| childless_nav(up_nav));
        let leading_childless = self
            .anchor_semantics
            .check_leading_anchor(&items)
            .map(childless_nav);
        let childless = match (trailing_childless, leading_childless) {
            (Some(a), Some(b)) => Some(tightest_childless(a, b)),
            (a, b) => a.or(b),
        };
        let skip_target = if let Some(nav) = childless {
            let label = self.fresh_label();
            self.instructions
                .push(MatchIR::epsilon(label, final_exit).nav(nav).into());
            label
        } else {
            final_exit
        };
        // A body of anchors alone consumes no child, so it is the zero-width
        // path and nothing else: the childless assertion is the whole
        // constraint. Compiling the descend/ascend pair around an empty
        // match would emit a bare ascent (`verify` rightly rejects it).
        let items_entry = if items_have_patterns(&items) {
            let up_label = self.fresh_label();
            let entry = self.compile_seq_items(SeqItemsCtx {
                items: &items,
                exit: up_label,
                is_inside_node: true,
                first_nav: None,
                capture: CaptureEffects::default(),
                skip_exit: Some(SkipExit::To(skip_target)),
            });
            self.instructions
                .push(MatchIR::epsilon(up_label, final_exit).nav(up_nav).into());
            entry
        } else {
            skip_target
        };

        let mut entry_match = MatchIR::epsilon(entry, items_entry)
            .nav(nav)
            .node_kind(node_kind)
            .neg_fields(neg_fields)
            .prepend_effects(capture.pre)
            .append_effects(entry_effects);
        if let Some(p) = predicate {
            entry_match = entry_match.predicate(p);
        }
        self.emit_match(entry_match);

        entry
    }

    /// Post-effects (like `EndEnum`) must run after children complete, not right after
    /// matching the parent node. Returns `exit` unchanged when `post` is empty.
    fn emit_trailing_effects_exit(&mut self, exit: Label, post: Vec<EffectIR>) -> Label {
        if post.is_empty() {
            exit
        } else {
            self.emit_effects_epsilon(exit, post, CaptureEffects::default())
        }
    }
}

/// Whether any item — descending through sequence groups — is a pattern that
/// consumes a child. A body failing this is anchors alone: one zero-width
/// match with no descent into the child list.
fn items_have_patterns(items: &[ast::SeqItem]) -> bool {
    items.iter().any(|item| match item {
        ast::SeqItem::Pattern(Pattern::SeqPattern(seq)) => {
            let inner: Vec<_> = seq.items().collect();
            items_have_patterns(&inner)
        }
        ast::SeqItem::Pattern(_) => true,
        ast::SeqItem::Anchor(_) => false,
    })
}

/// The zero-width counterpart of an anchor's constrained nav: a trailing
/// anchor's `Up*` lastness mode or a leading anchor's `Down*` entry mode.
fn childless_nav(anchor_nav: Nav) -> Nav {
    match anchor_nav {
        Nav::UpSkipTrivia(_) | Nav::DownSkip => Nav::ChildlessSkipTrivia,
        Nav::UpSkipExtras(_) | Nav::DownSkipExtras => Nav::ChildlessSkipExtras,
        Nav::UpExact(_) | Nav::DownExact => Nav::ChildlessExact,
        _ => {
            unreachable!("an anchor always lowers to a constrained Up or Down, got {anchor_nav:?}")
        }
    }
}

/// The stricter of two childless checks. Their admitted-child sets nest
/// (`Exact` ⊂ `SkipExtras` ⊂ `SkipTrivia`), so a node passing the tighter
/// check passes the looser one — asserting both collapses to asserting one.
fn tightest_childless(a: Nav, b: Nav) -> Nav {
    let rank = |nav: Nav| match nav {
        Nav::ChildlessExact => 0,
        Nav::ChildlessSkipExtras => 1,
        Nav::ChildlessSkipTrivia => 2,
        _ => unreachable!("only childless navs are ranked, got {nav:?}"),
    };
    if rank(a) <= rank(b) { a } else { b }
}

impl NfaBuilder<'_> {
    pub(super) fn compile_token_pattern(
        &mut self,
        node: &ast::TokenPattern,
        ctx: PatternCtx,
    ) -> Label {
        let PatternCtx {
            exit,
            nav: nav_override,
            capture,
            value: _,
        } = ctx;
        let entry = self.fresh_label();
        let nav = nav_override.unwrap_or(Nav::Next);

        let node_kind = match node.value() {
            Some(token) => self.resolve_anonymous_node_kind(&unescape(token.text()).0),
            None => NodeKindConstraint::Any, // `_` wildcard matches any node
        };

        self.emit_match(
            MatchIR::epsilon(entry, exit)
                .nav(nav)
                .node_kind(node_kind)
                .prepend_effects(capture.pre)
                .append_effects(capture.post),
        );

        entry
    }

    /// Resolve a reference to the `DefId` of its target definition.
    pub(super) fn resolve_ref_def_id(&self, r: &ast::DefRef) -> DefId {
        let name_token = r.name().expect("validated reference must have a name");
        self.ctx
            .analysis
            .dependency_analysis
            .def_id_for_name(self.ctx.analysis.interner, name_token.text())
            .expect("analyzed reference must resolve to a definition")
    }

    /// Whether this pattern is a reference (possibly captured) to a nullable
    /// definition — one whose body can match zero nodes. Such references are
    /// skippable items: their bodies inline at the call site so the zero-width
    /// path exits like an inline `?` (see [`compile_ref_inline`](Self::compile_ref_inline)).
    pub(super) fn is_nullable_ref_item(&self, pattern: &Pattern) -> bool {
        let inner = match pattern {
            Pattern::CapturedPattern(cap) => {
                let Some(inner) = cap.inner() else {
                    return false;
                };
                inner
            }
            other => other.clone(),
        };
        let Pattern::DefRef(r) = &inner else {
            return false;
        };
        self.nullable_defs.contains(&self.resolve_ref_def_id(r))
    }

    /// Whether a pattern can match zero nodes (see `analyze::nullability`).
    pub(super) fn pattern_is_nullable(&self, pattern: &Pattern) -> bool {
        crate::compiler::analyze::nullability::pattern_nullable(
            pattern,
            &self.nullable_defs,
            self.ctx.analysis.dependency_analysis,
            self.ctx.analysis.interner,
        )
    }

    /// A sequence item that may consume nothing: a skippable quantifier, a
    /// reference to a nullable definition, a group of such items, or an
    /// alternation with a nullable branch.
    pub(super) fn is_skippable_item(&self, pattern: &Pattern) -> bool {
        self.pattern_is_nullable(pattern)
    }

    /// [`pattern_owns_iteration`] extended to nullable references: the inlined
    /// body stands in for the item, so iteration ownership is the body's — a
    /// quantifier-rooted body owns its sibling search, and wrapping it in a
    /// position search would double-search.
    pub(super) fn item_owns_iteration(&self, pattern: &Pattern) -> bool {
        if pattern_owns_iteration(pattern) {
            return true;
        }
        self.is_nullable_ref_item(pattern) && self.body_owns_iteration(pattern)
    }

    /// Iteration ownership of the pattern a reference inlines to, following
    /// alias bodies. Terminates because pure-alias cycles never consume and
    /// are rejected by the recursion rules.
    fn body_owns_iteration(&self, pattern: &Pattern) -> bool {
        let inner = match pattern {
            Pattern::CapturedPattern(cap) => {
                let Some(inner) = cap.inner() else {
                    return false;
                };
                inner
            }
            other => other.clone(),
        };
        let Pattern::DefRef(r) = &inner else {
            return pattern_owns_iteration(&inner);
        };
        let def_id = self.resolve_ref_def_id(r);
        let name = self
            .ctx
            .analysis
            .interner
            .resolve(self.ctx.analysis.dependency_analysis.def_name_sym(def_id));
        let body = self
            .ctx
            .symbol_table
            .body(name)
            .expect("analyzed definition has a body");
        self.body_owns_iteration(body)
    }

    /// Compile a reference with capture effects.
    ///
    /// A reference to a nullable definition (body can match zero nodes) inlines
    /// the body at the call site: a real call's zero-width return would resume
    /// at a return address whose navigation assumes the candidate was consumed,
    /// stepping over an unmatched node. Inlining lets the ordinary skip-path
    /// machinery (checkpoint cursor restore, split exits) apply unchanged, so a
    /// reference matches exactly like its body written inline.
    ///
    /// Everything else compiles as a call ([`compile_ref_call`](Self::compile_ref_call)).
    pub(super) fn compile_ref(
        &mut self,
        r: &ast::DefRef,
        ctx: PatternCtx,
        field_override: Option<NodeFieldId>,
    ) -> Label {
        let def_id = self.resolve_ref_def_id(r);
        if self.nullable_defs.contains(&def_id) {
            // A nullable body has arity Many, which field values reject
            // upstream ("field cannot match a sequence").
            assert!(
                field_override.is_none(),
                "field-constrained reference to a nullable definition must be rejected by analysis"
            );
            let PatternCtx {
                exit,
                nav,
                capture,
                value,
            } = ctx;
            return self.compile_ref_inline(
                def_id,
                SplitExits {
                    match_exit: exit,
                    skip_exit: SkipExit::To(exit),
                },
                nav,
                capture,
                value,
            );
        }
        self.compile_ref_call(def_id, ctx, field_override, CalleeVariant::Full, false)
    }

    /// Compile a reference as a `Call` to the definition's standalone body.
    ///
    /// Call-site scoping: the caller decides whether to wrap with Struct/EndStruct based on
    /// whether the ref is captured and the called definition returns a struct.
    ///
    /// - Captured ref returning struct: `Struct → Call → EndStruct → Set → exit`
    /// - Captured ref returning scalar: `Call → Set → exit`
    /// - Bare ref returning output effects: `SuppressBegin → Call → SuppressEnd → exit`
    ///   (matches structurally, output discarded)
    /// - Bare ref to a void or node-scalar definition: `Call → exit`
    ///   (nothing to discard)
    ///
    /// `keep_value` forces the consumed lowering even with no consumer effect
    /// at this site: the callee's pending value must survive the call — it is
    /// the caller's own return value (a quantifier at a definition's root).
    fn compile_ref_call(
        &mut self,
        def_id: DefId,
        ctx: PatternCtx,
        field_override: Option<NodeFieldId>,
        callee_variant: CalleeVariant,
        keep_value: bool,
    ) -> Label {
        let PatternCtx {
            exit,
            nav: nav_override,
            capture,
            value: _,
        } = ctx;

        // Inside the trust boundary: `def_id_for_name` only yields DefIds for
        // symbol-table definitions, and `assert_all_definitions_processed` makes
        // `def_output` total over those — so `build_ir` registered a label for
        // every one. A miss is a desynced `def_output`/`def_entries`, our bug.
        let target = match callee_variant {
            CalleeVariant::Full => *self
                .def_entries
                .get(&def_id)
                .expect("every analyzed DefId has a def_entries label"),
            CalleeVariant::Consuming => self.compile_consuming_def(def_id),
        };
        let callee = CalleeEntry(target);

        let def_output_id = self.ctx.analysis.type_analysis.expect_def_output(def_id);
        let def_output_shape = self
            .ctx
            .analysis
            .type_analysis
            .expect_type_shape(def_output_id);
        let is_captured = post_consumes_call_value(&capture.post) || keep_value;
        let lowering = self.ref_call_lowering(def_output_shape, is_captured);

        let nav = nav_override.unwrap_or(Nav::Stay);

        // Call instructions cannot carry effects, so emit epsilon if needed.
        let call_entry = match lowering {
            RefLowering::ScopedCapture => {
                // Struct isolates the definition's internal captures before the Set.
                let set_step =
                    self.emit_effects_epsilon(exit, capture.post, CaptureEffects::default());
                let struct_close_step = self.emit_struct_close_step(set_step);
                let call_label =
                    self.emit_call(nav, field_override, ReturnAddr(struct_close_step), callee);
                self.emit_struct_step(call_label)
            }
            RefLowering::CapturedValue => {
                let return_addr =
                    self.emit_effects_epsilon(exit, capture.post, CaptureEffects::default());
                self.emit_call(nav, field_override, ReturnAddr(return_addr), callee)
            }
            RefLowering::SuppressedCall => {
                // Suppress bracket keeps the structural match but discards the
                // definition's output effects, matching the void that inference
                // assigns to a bare reference. Non-consuming post effects (an
                // enclosing variant's EnumClose, a scope close) run after the
                // bracket, outside the discarded region.
                let mut close_effects = vec![EffectIR::suppress_end()];
                close_effects.extend(capture.post);
                let suppress_end =
                    self.emit_effects_epsilon(exit, close_effects, CaptureEffects::default());
                let call_label =
                    self.emit_call(nav, field_override, ReturnAddr(suppress_end), callee);
                self.emit_effects_epsilon(
                    call_label,
                    vec![EffectIR::suppress_begin()],
                    CaptureEffects::default(),
                )
            }
            RefLowering::PlainCall => {
                // Void and node-scalar definitions emit no output effects in
                // their bodies; the call needs no bracket. Enclosing-scope post
                // effects still run after it.
                let return_addr = if capture.post.is_empty() {
                    exit
                } else {
                    self.emit_effects_epsilon(exit, capture.post, CaptureEffects::default())
                };
                self.emit_call(nav, field_override, ReturnAddr(return_addr), callee)
            }
        };

        if capture.pre.is_empty() {
            return call_entry;
        }

        // Wrap with pre-effects epsilon (e.g., Enum for enum alternations)
        self.emit_effects_epsilon(call_entry, capture.pre, CaptureEffects::default())
    }

    fn ref_call_lowering(&self, def_output_shape: &TypeShape, is_captured: bool) -> RefLowering {
        if is_captured {
            if matches!(def_output_shape, TypeShape::Struct(_)) {
                return RefLowering::ScopedCapture;
            }

            return RefLowering::CapturedValue;
        }

        // References are opaque: a bare reference matches structurally and its
        // output is suppressed (inference types it void). Void and node-scalar
        // definitions emit no output effects in their bodies, so there is
        // nothing to bracket.
        if matches!(def_output_shape, TypeShape::Void | TypeShape::Node) {
            return RefLowering::PlainCall;
        }

        RefLowering::SuppressedCall
    }

    /// Inline a nullable definition's body at the reference site.
    ///
    /// The lowering mirrors [`ref_call_lowering`](Self::ref_call_lowering) with
    /// the body substituted for the `Call`:
    ///
    /// - Captured ref returning struct: `Struct → body → EndStruct → Set → exit(s)`
    /// - Captured ref returning scalar: `body → Set → exit(s)`
    /// - Bare ref: body compiled under suppression (compile-time — no
    ///   `SuppressBegin`/`SuppressEnd` brackets needed)
    ///
    /// The body routes through [`compile_skippable_with_exits`](Self::compile_skippable_with_exits),
    /// so its zero-width path exits to `skip_exit` with the checkpoint-restored
    /// cursor — exactly the inline `?` semantics. Single-exit callers pass the
    /// same label for both exits; the paths still differ in cursor state.
    pub(super) fn compile_ref_inline(
        &mut self,
        def_id: DefId,
        exits: SplitExits,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
        value: bool,
    ) -> Label {
        self.compile_ref_inline_in(def_id, exits, nav_override, capture, value)
    }

    /// [`compile_ref_inline`](Self::compile_ref_inline) with `keep_value`: the
    /// body's pending value survives with no consumer effect at this site (see
    /// [`compile_ref_call`](Self::compile_ref_call)).
    pub(super) fn compile_ref_inline_keep_value(
        &mut self,
        def_id: DefId,
        exits: SplitExits,
        nav_override: Option<Nav>,
    ) -> Label {
        self.compile_ref_inline_in(def_id, exits, nav_override, CaptureEffects::default(), true)
    }

    /// Call a definition keeping its pending value alive (no consumer effect).
    pub(super) fn compile_ref_call_keep_value(
        &mut self,
        def_id: DefId,
        exit: Label,
        nav_override: Option<Nav>,
        field_override: Option<NodeFieldId>,
    ) -> Label {
        self.compile_ref_call(
            def_id,
            PatternCtx {
                exit,
                nav: nav_override,
                capture: CaptureEffects::default(),
                value: false,
            },
            field_override,
            CalleeVariant::Full,
            true,
        )
    }

    fn compile_ref_inline_in(
        &mut self,
        def_id: DefId,
        exits: SplitExits,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
        keep_value: bool,
    ) -> Label {
        if self.inline_stack.contains(&def_id) {
            return self.compile_ref_guarded_call(def_id, exits, nav_override, capture);
        }

        let name = self
            .ctx
            .analysis
            .interner
            .resolve(self.ctx.analysis.dependency_analysis.def_name_sym(def_id));
        let body = self
            .ctx
            .symbol_table
            .body(name)
            .expect("analyzed definition has a body");

        let def_output_id = self.ctx.analysis.type_analysis.expect_def_output(def_id);
        let def_output_shape = self
            .ctx
            .analysis
            .type_analysis
            .expect_type_shape(def_output_id);
        let is_captured = post_consumes_call_value(&capture.post) || keep_value;
        let inline_scoped_capture = is_captured && matches!(def_output_shape, TypeShape::Struct(_));

        let SplitExits {
            match_exit,
            skip_exit,
        } = exits;
        let CaptureEffects { pre, post } = capture;

        self.inline_stack.push(def_id);
        let entry = if inline_scoped_capture {
            // Struct isolates the definition's internal captures before the
            // Set; both continuations close it (a zero-width body still
            // produced its row of skip-path values, e.g. `{x: null}`).
            let end = ScopeCloseEffects {
                leading: &[],
                capture: &post,
                outer: &[],
            };
            let close_match = self.emit_struct_close_step_with_effects(end, match_exit);
            let close_skip = match skip_exit {
                SkipExit::To(skip) if skip == match_exit => SkipExit::To(close_match),
                SkipExit::To(skip) => {
                    SkipExit::To(self.emit_struct_close_step_with_effects(end, skip))
                }
                SkipExit::Fail => SkipExit::Fail,
            };
            let (body_match_exit, def_span) = self.bracket_def_body_exit(body, close_match);
            let body_skip_exit = match close_skip {
                SkipExit::To(skip) if skip == close_match => SkipExit::To(body_match_exit),
                SkipExit::To(skip) => SkipExit::To(self.bracket_def_body_exit(body, skip).0),
                SkipExit::Fail => SkipExit::Fail,
            };
            let body_entry = self.with_scope(def_output_id, |this| {
                this.compile_skippable_with_exits(
                    body,
                    SplitExits {
                        match_exit: body_match_exit,
                        skip_exit: body_skip_exit,
                    },
                    nav_override,
                    CaptureEffects::default(),
                    false,
                )
            });
            let body_entry = self.wrap_def_body_entry(body_entry, def_span);
            self.emit_struct_step_with_pre(body_entry, pre)
        } else if is_captured {
            // Scalar-valued (enum) body: it leaves its value pending; the
            // consumer chain runs after it on either continuation.
            let set_match = self.emit_effects_if_nonempty(match_exit, post.clone());
            let set_skip = match skip_exit {
                SkipExit::To(skip) if skip == match_exit => SkipExit::To(set_match),
                SkipExit::To(skip) => SkipExit::To(self.emit_effects_if_nonempty(skip, post)),
                SkipExit::Fail => SkipExit::Fail,
            };
            let (body_match_exit, def_span) = self.bracket_def_body_exit(body, set_match);
            let body_skip_exit = match set_skip {
                SkipExit::To(skip) if skip == set_match => SkipExit::To(body_match_exit),
                SkipExit::To(skip) => SkipExit::To(self.bracket_def_body_exit(body, skip).0),
                SkipExit::Fail => SkipExit::Fail,
            };
            let body_entry = self.with_scope(def_output_id, |this| {
                this.compile_skippable_with_exits(
                    body,
                    SplitExits {
                        match_exit: body_match_exit,
                        skip_exit: body_skip_exit,
                    },
                    nav_override,
                    CaptureEffects::default(),
                    true,
                )
            });
            let body_entry = self.wrap_def_body_entry(body_entry, def_span);
            self.wrap_entry_pre(body_entry, pre)
        } else {
            // Bare reference: opaque, so the body compiles structurally.
            // Suppression is compile-time here — captures are inert and
            // alternations tag nothing — which matches the void that
            // inference assigns without any runtime discard brackets.
            // Non-consuming post effects (an enclosing scope's close) run
            // after the body, outside the suppressed region.
            let end_match = self.emit_effects_if_nonempty(match_exit, post.clone());
            let end_skip = match skip_exit {
                SkipExit::To(skip) if skip == match_exit => SkipExit::To(end_match),
                SkipExit::To(skip) => SkipExit::To(self.emit_effects_if_nonempty(skip, post)),
                SkipExit::Fail => SkipExit::Fail,
            };
            let (body_match_exit, def_span) = self.bracket_def_body_exit(body, end_match);
            let body_skip_exit = match end_skip {
                SkipExit::To(skip) if skip == end_match => SkipExit::To(body_match_exit),
                SkipExit::To(skip) => SkipExit::To(self.bracket_def_body_exit(body, skip).0),
                SkipExit::Fail => SkipExit::Fail,
            };
            let body_entry = self.with_suppression(|this| {
                this.compile_skippable_with_exits(
                    body,
                    SplitExits {
                        match_exit: body_match_exit,
                        skip_exit: body_skip_exit,
                    },
                    nav_override,
                    CaptureEffects::default(),
                    false,
                )
            });
            let body_entry = self.wrap_def_body_entry(body_entry, def_span);
            self.wrap_entry_pre(body_entry, pre)
        };
        self.inline_stack.pop();

        entry
    }

    /// A nullable reference back into a definition currently being compiled —
    /// a consuming-position cycle through the def's own body, e.g.
    /// `A = (x (A) (y))?`. Inlining would not terminate, so fall back to a
    /// real call plus a zero-width bypass.
    ///
    /// The bypass is ordered first: the call's own zero-width return leaves
    /// the cursor on the unconsumed candidate, so any match it finds is also
    /// found — with an honest cursor and possibly an earlier follower
    /// candidate — via the bypass. Trying the call first would let those
    /// mis-navigated results shadow better ones; trying the bypass first makes
    /// the call's zero-width path harmless (its continuations are a subset).
    fn compile_ref_guarded_call(
        &mut self,
        def_id: DefId,
        exits: SplitExits,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        let SplitExits {
            match_exit,
            skip_exit,
        } = exits;
        // Captured references inside recursive cycles are rejected upstream,
        // so `post` holds only non-consuming effects (enclosing-scope closes)
        // that must run on both paths.
        let CaptureEffects { pre, post } = capture;

        let skip = match skip_exit {
            SkipExit::To(skip) => Some(self.emit_effects_if_nonempty(skip, post.clone())),
            SkipExit::Fail => None,
        };
        let call_entry = self.compile_ref_call(
            def_id,
            PatternCtx {
                exit: match_exit,
                nav: nav_override,
                capture: CaptureEffects::new_post(post),
                value: false,
            },
            None,
            CalleeVariant::Consuming,
            false,
        );
        let entry = match skip {
            Some(skip) => self.emit_branch_epsilon(
                BranchTargets {
                    prefer: call_entry,
                    other: skip,
                },
                Greediness::NonGreedy,
            ),
            // Pruned: the cycle must consume — no zero-width bypass.
            None => call_entry,
        };
        self.wrap_entry_pre(entry, pre)
    }

    pub(super) fn compile_field(&mut self, field: &ast::FieldPattern, ctx: PatternCtx) -> Label {
        let ctx = self.bracket_field_ctx(field, ctx);
        let PatternCtx {
            exit,
            nav: nav_override,
            capture,
            value: value_context,
        } = ctx;
        let value = field
            .value()
            .expect("validated field pattern must have a value");

        let node_field = self.resolve_field(field);

        if let Pattern::DefRef(r) = &value {
            let value_ctx = PatternCtx {
                exit,
                nav: nav_override,
                capture,
                value: value_context,
            };
            let value_ctx = self.bracket_pattern_ctx(&value, value_ctx);
            return self.compile_ref(r, value_ctx, node_field);
        }

        // Alternations, sequences, and quantified patterns emit an epsilon entry and
        // cannot carry a field constraint directly — the field must go on a wrapper
        // that navigates first, then lets the epsilon branch under it.
        if let Some(field_id) = node_field
            && Self::field_value_needs_wrapper(&value)
        {
            let value_ctx = PatternCtx {
                exit,
                nav: nav_override,
                capture,
                value: value_context,
            };
            return self.compile_wrapped_field_value(&value, value_ctx, field_id);
        }

        let value_entry = self.dispatch_pattern(
            &value,
            PatternCtx {
                exit,
                nav: nav_override,
                capture,
                value: value_context,
            },
        );

        self.attach_field_to_entry_or_wrap(value_entry, node_field)
    }

    fn bracket_field_ctx(&mut self, field: &ast::FieldPattern, ctx: PatternCtx) -> PatternCtx {
        let Some(id) = self.span_id(field.syntax(), SpanKind::Field) else {
            return ctx;
        };

        let PatternCtx {
            exit,
            nav,
            capture,
            value,
        } = ctx;
        PatternCtx {
            exit,
            nav,
            capture: capture.nest_span(EffectIR::span_start(id.0), EffectIR::span_end(id.0)),
            value,
        }
    }

    fn field_value_needs_wrapper(value: &Pattern) -> bool {
        matches!(
            value,
            Pattern::Union(_)
                | Pattern::Enum(_)
                | Pattern::SeqPattern(_)
                | Pattern::QuantifiedPattern(_)
        )
    }

    fn compile_wrapped_field_value(
        &mut self,
        value: &Pattern,
        ctx: PatternCtx,
        field_id: NodeFieldId,
    ) -> Label {
        let PatternCtx {
            exit,
            nav,
            capture,
            value: value_context,
        } = ctx;
        let value_entry = self.dispatch_pattern(
            value,
            PatternCtx {
                exit,
                nav: None,
                capture,
                value: value_context,
            },
        );

        let entry = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(entry, value_entry)
                .nav(nav.unwrap_or(Nav::Stay))
                .node_field(Some(field_id))
                .into(),
        );
        entry
    }

    fn attach_field_to_entry_or_wrap(
        &mut self,
        value_entry: Label,
        node_field: Option<NodeFieldId>,
    ) -> Label {
        if let Some(field_id) = node_field {
            if let Some(instr) = self
                .instructions
                .iter_mut()
                .find(|i| i.label() == value_entry)
                && let InstructionIR::Match(m) = instr
                && m.node_field.is_none()
            {
                m.node_field = Some(field_id);
                return value_entry;
            }

            // Fallback for patterns whose entry instruction couldn't accept the field;
            // Stay because the value was already compiled with navigation.
            let entry = self.fresh_label();
            self.instructions.push(
                MatchIR::epsilon(entry, value_entry)
                    .node_field(field_id)
                    .into(),
            );
            return entry;
        }

        value_entry
    }

    /// Compile a captured pattern, dispatching on its capture mechanism — the
    /// single source of truth shared with inference (#420) — so the effects we
    /// emit always match the declared type.
    ///
    /// `exits` selects single- or split-exit lowering (see [`CaptureExits`]). The
    /// ordinary capture path ([`dispatch_pattern`](Self::dispatch_pattern)) and
    /// the navigating-first-child skippable path
    /// ([`compile_skippable_with_exits`](Self::compile_skippable_with_exits)) both
    /// route here, so a mechanism can never be handled by one and dropped by the
    /// other (the drift behind #470 and the suppressive `@_` panic).
    ///
    /// Capture effects land on the innermost match / scope-close instruction:
    /// - Node:   inner_pattern[Node, Set] → exit
    /// - Struct: Struct → inner[…] → EndStruct+capture → exit
    /// - Array:  Arr → quantifier (with Push) → EndArr+capture → exit
    /// - Ref:    Call → Set epsilon → exit
    /// - Suppressive: SuppressBegin → inner → SuppressEnd → outer_effects → exit
    pub(super) fn compile_captured(
        &mut self,
        cap: &ast::CapturedPattern,
        inner_opt: Option<Pattern>,
        nav_override: Option<Nav>,
        outer_capture: CaptureEffects,
        exits: CaptureExits,
    ) -> Label {
        // Must precede mechanism dispatch: suppressive captures ignore the mechanism
        // entirely and must not build any capture effects for it. Inside an
        // already-suppressed region every capture is equally inert — inference
        // dropped its field, so emitting a Set would resolve against the wrong
        // scope (the panic behind #470).
        if cap.is_suppressive() || self.is_suppressed() {
            return self.compile_suppressive(
                inner_opt.as_ref(),
                nav_override,
                outer_capture,
                exits,
            );
        }

        // Classify the inner once — both the capture effects and the dispatch below
        // read it, so the declared type and the emitted effects can't disagree
        // (#420). `None` is a bare capture (`@x`), which captures the matched node.
        let mechanism = inner_opt.as_ref().map(|inner| {
            self.ctx.analysis.type_analysis.capture_kind(
                inner,
                self.ctx.analysis.dependency_analysis,
                self.ctx.analysis.interner,
            )
        });

        let (Some(inner), Some(mechanism)) = (inner_opt, mechanism) else {
            let capture_effects = self.build_capture_effects(cap, mechanism);
            return self.emit_effects_epsilon(exits.match_exit(), capture_effects, outer_capture);
        };

        let req = CaptureRequest::build(self, cap, &inner, nav_override, mechanism, outer_capture);

        match mechanism {
            // Array: Arr → quantifier (with Push) → EndArr+capture → exit(s).
            CaptureKind::Array => self.compile_array_capture(req, exits),

            // Struct scope: Struct → inner → EndStruct+capture → exit(s) (also empty `{}`).
            // Without the wrapper the Set lands on the raw inner node and both the
            // struct scope and the inner Sets are lost (#470).
            CaptureKind::Struct => self.compile_struct_capture(req, exits),

            // Node/Ref/PendingValue own no capture-site scope (their wrapper, if any, is
            // part of the inner). With split exits all three fold the capture onto the
            // body and recurse, letting the inner optional/star own the skip/match
            // split; that context always enters with empty `pre`, so the per-mechanism
            // single-exit handling (PendingValue's trailing Set, Node's bubble) is
            // unnecessary there.
            mechanism @ (CaptureKind::Node | CaptureKind::Ref | CaptureKind::PendingValue) => {
                match exits {
                    CaptureExits::Split {
                        match_exit,
                        skip_exit,
                    } => {
                        let CaptureRequest {
                            inner,
                            nav,
                            capture_effects,
                            outer_capture,
                        } = req;
                        let combined = outer_capture.with_post_values(capture_effects);
                        self.compile_skippable_with_exits(
                            inner,
                            SplitExits {
                                match_exit,
                                skip_exit,
                            },
                            nav,
                            combined,
                            false,
                        )
                    }
                    CaptureExits::Single(exit) => match mechanism {
                        CaptureKind::PendingValue => self.compile_setafter_capture(req, exit),
                        CaptureKind::Ref => self.compile_ref_capture(req, exit),
                        CaptureKind::Node => self.compile_node_capture(req, exit),
                        CaptureKind::Array | CaptureKind::Struct => {
                            unreachable!("scope mechanisms are handled above in compile_captured")
                        }
                    },
                }
            }
        }
    }

    /// Single-exit lowering for a `PendingValue` capture: the inner leaves the value
    /// pending (enum alternation or a named node forwarding a structured child).
    fn compile_setafter_capture(&mut self, req: CaptureRequest<'_>, exit: Label) -> Label {
        let CaptureRequest {
            inner,
            nav: nav_override,
            capture_effects,
            outer_capture,
        } = req;
        let CaptureEffects { pre, post } = outer_capture;
        let set_step =
            self.emit_effects_epsilon(exit, capture_effects, CaptureEffects::new_post(post));
        let inner_entry =
            self.dispatch_pattern(inner, PatternCtx::with_value(set_step, nav_override));
        // The enclosing variant's `Enum`-open (in `pre`) must run before the
        // inner produces its pending value; routing it through the trailing
        // `Set` step would drop it and unbalance the scope.
        self.wrap_entry_pre(inner_entry, pre)
    }

    /// Single-exit lowering for a `Ref` capture: hand the capture to the call
    /// site, which wraps Call/Return (and Struct/EndStruct for struct-returning
    /// definitions) to isolate the definition's internal captures before the Set.
    fn compile_ref_capture(&mut self, req: CaptureRequest<'_>, exit: Label) -> Label {
        let CaptureRequest {
            inner,
            nav: nav_override,
            capture_effects,
            outer_capture,
        } = req;
        let combined = outer_capture.with_post_values(capture_effects);
        self.dispatch_pattern(
            inner,
            PatternCtx {
                exit,
                nav: nav_override,
                capture: combined,
                value: false,
            },
        )
    }

    /// Single-exit lowering for a `Node` capture. Bubbling children, if any, set
    /// into the current scope alongside the capture.
    fn compile_node_capture(&mut self, req: CaptureRequest<'_>, exit: Label) -> Label {
        let inner_is_bubble = self
            .ctx
            .analysis
            .type_analysis
            .expect_pattern_result(req.inner)
            .flow
            .has_fields();
        if inner_is_bubble {
            return self.compile_bubble_with_node_capture(req, exit);
        }

        let CaptureRequest {
            inner,
            nav: nav_override,
            capture_effects,
            outer_capture,
        } = req;
        let combined = outer_capture.with_post_values(capture_effects);
        self.dispatch_pattern(
            inner,
            PatternCtx {
                exit,
                nav: nav_override,
                capture: combined,
                value: false,
            },
        )
    }

    /// Compile a suppressive capture (`@_`/`@_name`), or any capture inside an
    /// already-suppressed region: compile the inner structurally, in suppress
    /// mode, so nothing in the region emits output effects — captures are
    /// inert, alternations tag nothing, skip paths inject no nulls. That
    /// matches the `void` the type system infers without any runtime discard.
    /// The one output source that survives is a definition call (shared code),
    /// which the call site brackets itself (`RefLowering::SuppressedCall`).
    ///
    /// `outer.pre`/`outer.post` (e.g. an enum variant's `Enum`-open/`EndEnum`)
    /// belong to the enclosing scope and run outside the suppressed region.
    fn compile_suppressive(
        &mut self,
        inner: Option<&Pattern>,
        nav_override: Option<Nav>,
        outer_capture: CaptureEffects,
        exits: CaptureExits,
    ) -> Label {
        let CaptureEffects { pre, post } = outer_capture;

        let Some(inner) = inner else {
            // Bare `@_` never skips, so the match exit is the only continuation.
            let exit = exits.match_exit();
            if pre.is_empty() && post.is_empty() {
                return exit;
            }
            let entry = self.emit_effects_epsilon(exit, vec![], CaptureEffects::new_post(post));
            return self.wrap_entry_pre(entry, pre);
        };

        let inner_entry = match exits {
            CaptureExits::Single(exit) => {
                let end_label = if post.is_empty() {
                    exit
                } else {
                    self.emit_effects_epsilon(exit, vec![], CaptureEffects::new_post(post))
                };
                self.with_suppression(|this| {
                    this.dispatch_pattern(inner, PatternCtx::with_nav(end_label, nav_override))
                })
            }
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => {
                let (end_match, end_skip) = if post.is_empty() {
                    (match_exit, skip_exit)
                } else {
                    let end_match = self.emit_effects_epsilon(
                        match_exit,
                        vec![],
                        CaptureEffects::new_post(post.clone()),
                    );
                    let end_skip = match skip_exit {
                        SkipExit::To(skip) => SkipExit::To(self.emit_effects_epsilon(
                            skip,
                            vec![],
                            CaptureEffects::new_post(post),
                        )),
                        SkipExit::Fail => SkipExit::Fail,
                    };
                    (end_match, end_skip)
                };
                self.with_suppression(|this| {
                    this.compile_skippable_with_exits(
                        inner,
                        SplitExits {
                            match_exit: end_match,
                            skip_exit: end_skip,
                        },
                        nav_override,
                        CaptureEffects::default(),
                        false,
                    )
                })
            }
        };

        self.wrap_entry_pre(inner_entry, pre)
    }

    pub(super) fn resolve_anonymous_node_kind(&mut self, text: &str) -> NodeKindConstraint {
        let sym = self
            .ctx
            .analysis
            .interner
            .get(text)
            .expect("linked anonymous token must be interned");
        NodeKindConstraint::Anonymous(Some(self.ctx.analysis.grammar.expect_anonymous_kind(sym)))
    }

    /// Resolve a NodePattern to its node kind constraint.
    ///
    /// Returns `NodeKindConstraint::Named` with:
    /// - `None` for wildcard `(_)` (any named node)
    /// - `Some(id)` for specific types like `(identifier)`
    pub(super) fn resolve_node_kind(&mut self, node: &ast::NodePattern) -> NodeKindConstraint {
        if node.is_any() {
            return NodeKindConstraint::Named(None);
        }

        let type_token = node
            .kind_token()
            .expect("validated node pattern must have a kind token");
        if matches!(
            type_token.kind(),
            SyntaxKind::KwError | SyntaxKind::KwMissing
        ) {
            return NodeKindConstraint::Named(None);
        }
        let type_name = type_token.text();

        let sym = self
            .ctx
            .analysis
            .interner
            .get(type_name)
            .expect("linked named node kind must be interned");
        NodeKindConstraint::Named(Some(self.ctx.analysis.grammar.expect_named_kind(sym)))
    }

    /// Resolve a field pattern to its grammar `NodeFieldId`.
    pub(super) fn resolve_field(&mut self, field: &ast::FieldPattern) -> Option<NodeFieldId> {
        let name_token = field
            .name()
            .expect("validated field pattern must have a field name");
        let field_name = name_token.text();
        Some(self.resolve_field_by_name(field_name))
    }

    /// Resolve a field name to its grammar `NodeFieldId`.
    pub(super) fn resolve_field_by_name(&mut self, field_name: &str) -> NodeFieldId {
        let sym = self
            .ctx
            .analysis
            .interner
            .get(field_name)
            .expect("linked field name must be interned");
        self.ctx.analysis.grammar.expect_field(sym)
    }

    pub(super) fn collect_neg_fields(&mut self, node: &ast::NodePattern) -> Vec<NodeFieldId> {
        node.syntax()
            .children()
            .filter_map(ast::NegatedField::cast)
            .map(|nf| {
                let name = nf
                    .name()
                    .expect("validated negated field must have a field name");
                self.resolve_field_by_name(name.text())
            })
            .collect()
    }

    /// Compile a predicate from AST to IR.
    ///
    /// Returns `Some(PredicateIR)` if the node has a valid predicate, `None` otherwise.
    pub(super) fn compile_predicate(&mut self, node: &ast::NodePattern) -> Option<PredicateIR> {
        let pred = node.predicate()?;
        let op = lower_predicate_op(pred.operator()?);

        if let Some(str_token) = pred.string_value() {
            return Some(PredicateIR::string(op, unescape(str_token.text()).0));
        }

        if let Some(regex) = pred.regex() {
            let text: String = regex.syntax().text().into();
            let without_prefix = text
                .strip_prefix('/')
                .expect("regex token is '/'-delimited after parse");
            let pattern = without_prefix
                .strip_suffix('/')
                .expect("regex token is '/'-delimited after parse");
            return Some(PredicateIR::regex(op, pattern));
        }

        None
    }
}

fn lower_predicate_op(op: ast::PredicateOperator) -> PredicateOp {
    match op {
        ast::PredicateOperator::Eq => PredicateOp::Eq,
        ast::PredicateOperator::Ne => PredicateOp::Ne,
        ast::PredicateOperator::StartsWith => PredicateOp::StartsWith,
        ast::PredicateOperator::EndsWith => PredicateOp::EndsWith,
        ast::PredicateOperator::Contains => PredicateOp::Contains,
        ast::PredicateOperator::RegexMatch => PredicateOp::RegexMatch,
        ast::PredicateOperator::RegexNoMatch => PredicateOp::RegexNoMatch,
    }
}
