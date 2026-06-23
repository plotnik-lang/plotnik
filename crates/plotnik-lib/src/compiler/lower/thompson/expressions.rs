//! Expression compilation for leaf and wrapper patterns.
//!
//! Handles compilation of:
//! - Named nodes: `(identifier)`, `(call_expression ...)`
//! - Anonymous nodes: `"+"`, `_`
//! - References: `(Pattern)` (calls to other definitions)
//! - Field constraints: `name: pattern`
//! - Captured expressions: `@name`, `pattern @name`

use std::num::NonZeroU16;

use crate::bytecode::Nav;
use crate::compiler::core::TypeShape;
use crate::compiler::core::ast::{self, Pattern};
use crate::compiler::lower::ir::{
    EffectIR, InstructionIR, Label, MatchIR, NodeKindConstraint, PredicateIR,
};

use crate::compiler::core::CaptureMechanism;

use super::Compiler;
use super::capture::{CaptureEffects, ExprCtx};
use super::navigation::check_trailing_anchor;
use super::scope::{CaptureExits, CaptureRequest, SplitExits};
use super::sequences::SeqItemsCtx;

impl Compiler<'_> {
    pub(super) fn compile_node_pattern(&mut self, node: &ast::NodePattern, ctx: ExprCtx) -> Label {
        let ExprCtx {
            exit,
            nav: nav_override,
            capture,
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
                .pre_effects(capture.pre)
                .post_effects(capture.post);
            if let Some(p) = predicate {
                m = m.predicate(p);
            }
            return self.emit_match(m);
        }

        let (has_trailing_anchor, trailing_nav) =
            check_trailing_anchor(&items, self.ctx.symbol_table);

        // Emit Up instruction with appropriate strictness. A trailing anchor only
        // changes this ascent into a lastness check (`Up*`); the body itself
        // compiles like any node body, with `compile_seq_items` keeping the
        // last item's child search resumable so a lastness failure can retry.
        let up_nav = if has_trailing_anchor {
            trailing_nav.unwrap_or(Nav::UpSkipTrivia(1))
        } else {
            Nav::Up(1)
        };

        // Split capture.post: Node effects (and their Set) go on entry (need matched_node
        // right after match), other effects go on final_exit (after children processing).
        // Node capture effects use matched_node which is only valid immediately after the match,
        // before descending into children (which may clobber matched_node via backtracking).
        use crate::bytecode::EffectKind;

        let mut entry_effects = Vec::new();
        let mut exit_effects = Vec::new();
        let mut iter = capture.post.into_iter().peekable();
        while let Some(eff) = iter.next() {
            if eff.kind() == EffectKind::Node {
                entry_effects.push(eff);
                // Node is always paired with a following Set
                if iter.peek().is_some_and(|e| e.kind() == EffectKind::Set) {
                    entry_effects.push(
                        iter.next()
                            .expect("peek confirmed the iterator has a next element"),
                    );
                }
            } else {
                exit_effects.push(eff);
            }
        }

        // With items: nav[entry_effects] → items → Up → [exit_effects] → exit
        let final_exit = self.emit_post_effects_exit(exit, exit_effects);

        let up_label = self.fresh_label();
        let items_entry = self.compile_seq_items(SeqItemsCtx {
            items: &items,
            exit: up_label,
            is_inside_node: true,
            first_nav: None,
            capture: CaptureEffects::default(),
            skip_exit: Some(final_exit), // Skip exit bypasses Up when Down fails (childless node)
        });

        self.instructions
            .push(MatchIR::epsilon(up_label, final_exit).nav(up_nav).into());

        let mut entry_match = MatchIR::epsilon(entry, items_entry)
            .nav(nav)
            .node_kind(node_kind)
            .neg_fields(neg_fields)
            .pre_effects(capture.pre)
            .post_effects(entry_effects);
        if let Some(p) = predicate {
            entry_match = entry_match.predicate(p);
        }
        self.emit_match(entry_match);

        entry
    }

    /// Post-effects (like `EndEnum`) must run after children complete, not right after
    /// matching the parent node. Returns `exit` unchanged when `post` is empty.
    fn emit_post_effects_exit(&mut self, exit: Label, post: Vec<EffectIR>) -> Label {
        if post.is_empty() {
            exit
        } else {
            self.emit_effects_epsilon(exit, post, CaptureEffects::default())
        }
    }

    pub(super) fn compile_token_pattern(
        &mut self,
        node: &ast::TokenPattern,
        ctx: ExprCtx,
    ) -> Label {
        let ExprCtx {
            exit,
            nav: nav_override,
            capture,
        } = ctx;
        let entry = self.fresh_label();
        let nav = nav_override.unwrap_or(Nav::Next);

        let node_kind = match node.value() {
            Some(token) => self.resolve_anonymous_node_kind(token.text()),
            None => NodeKindConstraint::Any, // `_` wildcard matches any node
        };

        self.emit_match(
            MatchIR::epsilon(entry, exit)
                .nav(nav)
                .node_kind(node_kind)
                .pre_effects(capture.pre)
                .post_effects(capture.post),
        );

        entry
    }

    /// Compile a reference with capture effects.
    ///
    /// Call-site scoping: the caller decides whether to wrap with Struct/EndStruct based on
    /// whether the ref is captured and the called definition returns a struct.
    ///
    /// - Captured ref returning struct: `Struct → Call → EndStruct → Set → exit`
    /// - Captured ref returning scalar: `Call → Set → exit`
    /// - Uncaptured ref: `Call → exit` (def's Sets go to parent scope)
    pub(super) fn compile_ref(
        &mut self,
        r: &ast::Ref,
        ctx: ExprCtx,
        field_override: Option<NonZeroU16>,
    ) -> Label {
        let ExprCtx {
            exit,
            nav: nav_override,
            capture,
        } = ctx;
        let Some(name_token) = r.name() else {
            return exit;
        };
        let name = name_token.text();

        let Some(def_id) = self
            .ctx
            .dependency_analysis
            .def_id_for_name(self.ctx.interner, name)
        else {
            return exit;
        };

        // Inside the trust boundary: `def_id_for_name` only yields DefIds for
        // symbol-table definitions, and `assert_all_definitions_processed` makes
        // `def_output` total over those — so `build_ir` registered a label for
        // every one. A miss is a desynced `def_output`/`def_entries`, our bug.
        let &target = self
            .def_entries
            .get(&def_id)
            .expect("every analyzed DefId has a def_entries label");

        let def_output_id = self.ctx.type_ctx.def_output(def_id);
        let ref_returns_struct = def_output_id
            .and_then(|tid| self.ctx.type_ctx.type_shape(tid))
            .is_some_and(|shape| matches!(shape, TypeShape::Struct(_)));

        let is_captured = !capture.post.is_empty();
        let needs_scope = is_captured && ref_returns_struct;

        // An uncaptured recursive reference is opaque: inference types it Void, so
        // its captures must not bubble into the parent scope. Enum recursion
        // is the exception — inference forwards its enum value — so only the rest is
        // suppressed.
        let ref_returns_enum = def_output_id
            .and_then(|tid| self.ctx.type_ctx.type_shape(tid))
            .is_some_and(|shape| matches!(shape, TypeShape::Enum(_)));
        let suppress_opaque_recursion = !is_captured
            && self.ctx.dependency_analysis.is_recursive_def(def_id)
            && !ref_returns_enum;

        let nav = nav_override.unwrap_or(Nav::Stay);

        // Call instructions don't have pre_effects, so emit epsilon if needed
        let call_entry = if needs_scope {
            // Struct isolates the definition's internal captures before the Set.
            let set_step = self.emit_effects_epsilon(exit, capture.post, CaptureEffects::default());
            let struct_close_step = self.emit_struct_close_step(set_step);
            let call_label = self.emit_call(nav, field_override, struct_close_step, target);
            self.emit_struct_step(call_label)
        } else if is_captured {
            let return_addr =
                self.emit_effects_epsilon(exit, capture.post, CaptureEffects::default());
            self.emit_call(nav, field_override, return_addr, target)
        } else if suppress_opaque_recursion {
            // Suppress bracket keeps the structural match but discards all effects,
            // matching the Void that inference assigns to uncaptured opaque recursion.
            let suppress_end = self.emit_effects_epsilon(
                exit,
                vec![EffectIR::suppress_end()],
                CaptureEffects::default(),
            );
            let call_label = self.emit_call(nav, field_override, suppress_end, target);
            self.emit_effects_epsilon(
                call_label,
                vec![EffectIR::suppress_begin()],
                CaptureEffects::default(),
            )
        } else {
            // Uncaptured ref: just Call → exit (def's Sets go to parent scope)
            self.emit_call(nav, field_override, exit, target)
        };

        if capture.pre.is_empty() {
            return call_entry;
        }

        // Wrap with pre-effects epsilon (e.g., Enum for enum alternations)
        self.emit_effects_epsilon(call_entry, capture.pre, CaptureEffects::default())
    }

    pub(super) fn compile_field(&mut self, field: &ast::FieldPattern, ctx: ExprCtx) -> Label {
        let ExprCtx {
            exit,
            nav: nav_override,
            capture,
        } = ctx;
        let Some(value) = field.value() else {
            return exit;
        };

        let node_field = self.resolve_field(field);

        if let Pattern::Ref(r) = &value {
            return self.compile_ref(
                r,
                ExprCtx {
                    exit,
                    nav: nav_override,
                    capture,
                },
                node_field,
            );
        }

        // Alternations, sequences, and quantified patterns emit an epsilon entry and
        // cannot carry a field constraint directly — the field must go on a wrapper
        // that navigates first, then lets the epsilon branch under it.
        let needs_wrapper = node_field.is_some()
            && matches!(
                &value,
                Pattern::Union(_)
                    | Pattern::Enum(_)
                    | Pattern::SeqPattern(_)
                    | Pattern::QuantifiedPattern(_)
            );

        if needs_wrapper {
            let value_entry = self.dispatch_pattern(
                &value,
                ExprCtx {
                    exit,
                    nav: None,
                    capture,
                },
            );

            let entry = self.fresh_label();
            self.instructions.push(
                MatchIR::epsilon(entry, value_entry)
                    .nav(nav_override.unwrap_or(Nav::Stay))
                    .node_field(node_field)
                    .into(),
            );
            return entry;
        }

        let value_entry = self.dispatch_pattern(
            &value,
            ExprCtx {
                exit,
                nav: nav_override,
                capture,
            },
        );

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

    /// Compile a captured expression, dispatching on its capture mechanism — the
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
        // entirely and must not build any capture effects for it.
        if cap.is_suppressive() {
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
            self.ctx.type_ctx.capture_mechanism(
                inner,
                self.ctx.dependency_analysis,
                self.ctx.interner,
            )
        });

        let capture_effects = self.build_capture_effects(cap, mechanism);

        let (Some(inner), Some(mechanism)) = (inner_opt, mechanism) else {
            return self.emit_effects_epsilon(exits.match_exit(), capture_effects, outer_capture);
        };

        match mechanism {
            // Array: Arr → quantifier (with Push) → EndArr+capture → exit(s).
            CaptureMechanism::Array => self.compile_array_capture(
                CaptureRequest {
                    inner: &inner,
                    nav: nav_override,
                    capture_effects,
                    outer_capture,
                },
                exits,
            ),

            // Struct scope: Struct → inner → EndStruct+capture → exit(s) (also empty `{}`).
            // Without the wrapper the Set lands on the raw inner node and both the
            // struct scope and the inner Sets are lost (#470).
            CaptureMechanism::StructScope => self.compile_struct_capture(
                CaptureRequest {
                    inner: &inner,
                    nav: nav_override,
                    capture_effects,
                    outer_capture,
                },
                exits,
            ),

            // Node/Ref/SetAfter own no capture-site scope (their wrapper, if any, is
            // part of the inner). With split exits all three fold the capture onto the
            // body and recurse, letting the inner optional/star own the skip/match
            // split; that context always enters with empty `pre`, so the per-mechanism
            // single-exit handling (SetAfter's trailing Set, Node's bubble) is
            // unnecessary there.
            mechanism @ (CaptureMechanism::Node
            | CaptureMechanism::Ref
            | CaptureMechanism::SetAfter) => match exits {
                CaptureExits::Split {
                    match_exit,
                    skip_exit,
                } => {
                    let combined = outer_capture.with_post_values(capture_effects);
                    self.compile_skippable_with_exits(
                        &inner,
                        SplitExits {
                            match_exit,
                            skip_exit,
                        },
                        nav_override,
                        combined,
                    )
                }
                CaptureExits::Single(exit) => {
                    let req = CaptureRequest {
                        inner: &inner,
                        nav: nav_override,
                        capture_effects,
                        outer_capture,
                    };
                    match mechanism {
                        CaptureMechanism::SetAfter => self.compile_setafter_capture(req, exit),
                        CaptureMechanism::Ref => self.compile_ref_capture(req, exit),
                        CaptureMechanism::Node => self.compile_node_capture(req, exit),
                        CaptureMechanism::Array | CaptureMechanism::StructScope => {
                            unreachable!("scope mechanisms are handled above in compile_captured")
                        }
                    }
                }
            },
        }
    }

    /// Single-exit lowering for a `SetAfter` capture: the inner leaves the value
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
        let inner_entry = self.dispatch_pattern(inner, ExprCtx::with_nav(set_step, nav_override));
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
            ExprCtx {
                exit,
                nav: nav_override,
                capture: combined,
            },
        )
    }

    /// Single-exit lowering for a `Node` capture. Bubbling children, if any, set
    /// into the current scope alongside the capture.
    fn compile_node_capture(&mut self, req: CaptureRequest<'_>, exit: Label) -> Label {
        let CaptureRequest {
            inner,
            nav: nav_override,
            capture_effects,
            outer_capture,
        } = req;
        let inner_is_bubble = self
            .ctx
            .type_ctx
            .pattern_result(inner)
            .expect("an analyzed capture inner has a pattern result")
            .flow
            .has_fields();
        if inner_is_bubble {
            return self.compile_bubble_with_node_capture(
                inner,
                exit,
                nav_override,
                None,
                capture_effects,
                outer_capture,
            );
        }
        let combined = outer_capture.with_post_values(capture_effects);
        self.dispatch_pattern(
            inner,
            ExprCtx {
                exit,
                nav: nav_override,
                capture: combined,
            },
        )
    }

    /// Compile a suppressive capture (`@_`/`@_name`): wrap the inner in
    /// SuppressBegin/SuppressEnd and emit no value. The suppress region brackets
    /// whatever the inner emits (its own `Set`s, a skipped optional's nulls) and
    /// discards it at runtime, matching the `void` the type system infers.
    ///
    /// With `Split` exits the inner's match/skip paths route to two SuppressEnd
    /// steps. `outer.pre` (e.g. an enum variant's `Enum`-open) runs before
    /// SuppressBegin in the enclosing scope, so the tag is produced and the later
    /// `EndEnum` matches.
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

        // SuppressEnd + outer post (e.g. an enum variant's EndEnum) closes each exit;
        // the inner is compiled with NO capture effects.
        let inner_entry = match exits {
            CaptureExits::Single(exit) => {
                let end_label = self.emit_effects_epsilon(
                    exit,
                    vec![EffectIR::suppress_end()],
                    CaptureEffects::new_post(post),
                );
                self.dispatch_pattern(inner, ExprCtx::with_nav(end_label, nav_override))
            }
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => {
                let end_match = self.emit_effects_epsilon(
                    match_exit,
                    vec![EffectIR::suppress_end()],
                    CaptureEffects::new_post(post.clone()),
                );
                let end_skip = self.emit_effects_epsilon(
                    skip_exit,
                    vec![EffectIR::suppress_end()],
                    CaptureEffects::new_post(post),
                );
                self.compile_skippable_with_exits(
                    inner,
                    SplitExits {
                        match_exit: end_match,
                        skip_exit: end_skip,
                    },
                    nav_override,
                    CaptureEffects::default(),
                )
            }
        };

        let begin_entry = self.emit_effects_epsilon(
            inner_entry,
            vec![EffectIR::suppress_begin()],
            CaptureEffects::default(),
        );
        self.wrap_entry_pre(begin_entry, pre)
    }

    pub(super) fn resolve_anonymous_node_kind(&mut self, text: &str) -> NodeKindConstraint {
        let Some(sym) = self.ctx.interner.get(text) else {
            return NodeKindConstraint::Anonymous(None);
        };
        self.ctx
            .grammar
            .resolve_anonymous_kind(sym)
            .and_then(|id| NonZeroU16::new(id.get()))
            .map_or(NodeKindConstraint::Anonymous(None), |id| {
                NodeKindConstraint::Anonymous(Some(id))
            })
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

        let Some(type_token) = node.kind_token() else {
            return NodeKindConstraint::Named(None);
        };
        let type_name = type_token.text();

        let Some(sym) = self.ctx.interner.get(type_name) else {
            return NodeKindConstraint::Named(None);
        };
        self.ctx
            .grammar
            .resolve_named_kind(sym)
            .and_then(|id| NonZeroU16::new(id.get()))
            .map_or(NodeKindConstraint::Named(None), |id| {
                NodeKindConstraint::Named(Some(id))
            })
    }

    /// Resolve a field expression to its grammar `NodeFieldId`.
    pub(super) fn resolve_field(&mut self, field: &ast::FieldPattern) -> Option<NonZeroU16> {
        let name_token = field.name()?;
        let field_name = name_token.text();
        self.resolve_field_by_name(field_name)
    }

    /// Resolve a field name to its grammar `NodeFieldId`.
    ///
    /// An unknown field is left unconstrained (`None`).
    pub(super) fn resolve_field_by_name(&mut self, field_name: &str) -> Option<NonZeroU16> {
        self.ctx
            .interner
            .get(field_name)
            .and_then(|sym| self.ctx.grammar.resolve_field(sym))
            .and_then(|id| NonZeroU16::new(id.get()))
    }

    pub(super) fn collect_neg_fields(&mut self, node: &ast::NodePattern) -> Vec<u16> {
        node.syntax()
            .children()
            .filter_map(ast::NegatedField::cast)
            .filter_map(|nf| {
                let name = nf.name()?;
                self.resolve_field_by_name(name.text())
            })
            .map(|id| id.get())
            .collect()
    }

    /// Compile a predicate from AST to IR.
    ///
    /// Returns `Some(PredicateIR)` if the node has a valid predicate, `None` otherwise.
    pub(super) fn compile_predicate(&mut self, node: &ast::NodePattern) -> Option<PredicateIR> {
        let pred = node.predicate()?;
        let op = pred.operator()?;

        if let Some(str_token) = pred.string_value() {
            return Some(PredicateIR::string(op, str_token.text()));
        }

        if let Some(regex) = pred.regex() {
            // Get pattern text by stripping `/` delimiters from CST text
            let text: String = regex.syntax().text().into();
            let without_prefix = text.strip_prefix('/').unwrap_or(&text);
            let pattern = without_prefix.strip_suffix('/').unwrap_or(without_prefix);
            return Some(PredicateIR::regex(op, pattern));
        }

        None
    }
}
