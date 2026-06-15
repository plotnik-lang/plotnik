//! Expression compilation for leaf and wrapper patterns.
//!
//! Handles compilation of:
//! - Named nodes: `(identifier)`, `(call_expression ...)`
//! - Anonymous nodes: `"+"`, `_`
//! - References: `(Expr)` (calls to other definitions)
//! - Field constraints: `name: pattern`
//! - Captured expressions: `@name`, `pattern @name`

use std::num::NonZeroU16;

use crate::analyze::type_check::TypeShape;
use crate::bytecode::{EffectIR, InstructionIR, Label, MatchIR, NodeTypeIR, PredicateIR};
use crate::parser::ast::{self, Expr};
use plotnik_bytecode::Nav;
use plotnik_core::NodeType;

use crate::analyze::type_check::{CaptureMechanism, capture_mechanism};

use super::Compiler;
use super::capture::CaptureEffects;
use super::navigation::check_trailing_anchor;
use super::scope::CaptureExits;
use super::sequences::SeqItemsCtx;

impl Compiler<'_> {
    /// Compile a named node with capture effects.
    pub(super) fn compile_named_node_inner(
        &mut self,
        node: &ast::NamedNode,
        exit: Label,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        let entry = self.fresh_label();
        let node_type = self.resolve_node_type(node);
        let nav = nav_override.unwrap_or(Nav::Stay);

        // Collect items, negated fields, and predicate
        let items: Vec<_> = node.items().collect();
        let neg_fields = self.collect_neg_fields(node);
        let predicate = self.compile_predicate(node);

        // If no items, just match and exit (capture effects go here)
        if items.is_empty() {
            let mut m = MatchIR::epsilon(entry, exit)
                .nav(nav)
                .node_type(node_type)
                .neg_fields(neg_fields)
                .pre_effects(capture.pre)
                .post_effects(capture.post);
            if let Some(p) = predicate {
                m = m.predicate(p);
            }
            return self.emit_match(m);
        }

        // Determine Up navigation based on trailing anchor
        let (has_trailing_anchor, trailing_nav) =
            check_trailing_anchor(&items, self.ctx.symbol_table);

        // Emit Up instruction with appropriate strictness. A trailing anchor only
        // changes this ascent into a lastness check (`Up*`); the body itself
        // compiles like any node body, with `compile_seq_items_inner` keeping the
        // last item's child search resumable so a lastness failure can retry.
        let up_nav = if has_trailing_anchor {
            trailing_nav.unwrap_or(Nav::UpSkipTrivia(1))
        } else {
            Nav::Up(1)
        };

        // Split capture.post: Node/Text effects (and their Set) go on entry (need matched_node
        // right after match), other effects go on final_exit (after children processing).
        // Node/Text capture effects use matched_node which is only valid immediately after the match,
        // before descending into children (which may clobber matched_node via backtracking).
        use plotnik_bytecode::EffectOpcode;

        // Find Node/Text effects and their following Set effects (they come in pairs: Node Set)
        let mut entry_effects = Vec::new();
        let mut exit_effects = Vec::new();
        let mut iter = capture.post.into_iter().peekable();
        while let Some(eff) = iter.next() {
            if matches!(eff.opcode(), EffectOpcode::Node | EffectOpcode::Text) {
                entry_effects.push(eff);
                // Take the following Set if present (Node/Text are always followed by Set)
                if iter.peek().is_some_and(|e| e.opcode() == EffectOpcode::Set) {
                    entry_effects.push(iter.next().unwrap());
                }
            } else {
                exit_effects.push(eff);
            }
        }

        // With items: nav[entry_effects] → items → Up → [exit_effects] → exit
        let final_exit = self.emit_post_effects_exit(exit, exit_effects);

        let up_label = self.fresh_label();
        let items_entry = self.compile_seq_items_inner(SeqItemsCtx {
            items: &items,
            exit: up_label,
            is_inside_node: true,
            first_nav: None,
            capture: CaptureEffects::default(),
            skip_exit: Some(final_exit), // Skip exit bypasses Up when Down fails (childless node)
        });

        self.instructions
            .push(MatchIR::epsilon(up_label, final_exit).nav(up_nav).into());

        // Emit entry instruction with node_effects on post (executes right after match)
        let mut entry_match = MatchIR::epsilon(entry, items_entry)
            .nav(nav)
            .node_type(node_type)
            .neg_fields(neg_fields)
            .pre_effects(capture.pre)
            .post_effects(entry_effects);
        if let Some(p) = predicate {
            entry_match = entry_match.predicate(p);
        }
        self.emit_match(entry_match);

        entry
    }

    /// Emit post-effects on an epsilon step after the exit label.
    ///
    /// Post-effects (like EndEnum) must execute AFTER children complete, not after
    /// matching the parent node. This helper creates an epsilon step for the effects
    /// when needed, or returns the original exit if no effects.
    fn emit_post_effects_exit(&mut self, exit: Label, post: Vec<EffectIR>) -> Label {
        if post.is_empty() {
            exit
        } else {
            self.emit_effects_epsilon(exit, post, CaptureEffects::default())
        }
    }

    /// Compile an anonymous node with capture effects.
    pub(super) fn compile_anonymous_node_inner(
        &mut self,
        node: &ast::AnonymousNode,
        exit: Label,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        let entry = self.fresh_label();
        let nav = nav_override.unwrap_or(Nav::Next);

        // Extract literal value (Any for wildcard `_`, Anonymous for literals)
        let node_type = match node.value() {
            Some(token) => self.resolve_anonymous_node_type(token.text()),
            None => NodeTypeIR::Any, // `_` wildcard matches any node
        };

        self.emit_match(
            MatchIR::epsilon(entry, exit)
                .nav(nav)
                .node_type(node_type)
                .pre_effects(capture.pre)
                .post_effects(capture.post),
        );

        entry
    }

    /// Compile a reference with capture effects.
    ///
    /// Call-site scoping: the caller decides whether to wrap with Obj/EndObj based on
    /// whether the ref is captured and the called definition returns a struct.
    ///
    /// - Captured ref returning struct: `Obj → Call → EndObj → Set → exit`
    /// - Captured ref returning scalar: `Call → Set → exit`
    /// - Uncaptured ref: `Call → exit` (def's Sets go to parent scope)
    pub(super) fn compile_ref_inner(
        &mut self,
        r: &ast::Ref,
        exit: Label,
        nav_override: Option<Nav>,
        field_override: Option<NonZeroU16>,
        capture: CaptureEffects,
    ) -> Label {
        let Some(name_token) = r.name() else {
            return exit;
        };
        let name = name_token.text();

        let Some(def_id) = self.ctx.type_ctx.get_def_id(self.ctx.interner, name) else {
            return exit;
        };

        let Some(&target) = self.def_entries.get(&def_id) else {
            return exit;
        };

        // Check if the called definition returns a struct (needs scope isolation when captured)
        let def_type_id = self.ctx.type_ctx.get_def_type(def_id);
        let ref_returns_struct = def_type_id
            .and_then(|tid| self.ctx.type_ctx.get_type(tid))
            .is_some_and(|shape| matches!(shape, TypeShape::Struct(_)));

        // Determine if this is a captured ref that needs scope isolation
        let is_captured = !capture.post.is_empty();
        let needs_scope = is_captured && ref_returns_struct;

        // An uncaptured recursive reference is opaque: inference types it Void, so
        // its captures must not bubble into the parent scope. Tagged-union recursion
        // is the exception — inference forwards its enum value — so only the rest is
        // suppressed.
        let ref_returns_enum = def_type_id
            .and_then(|tid| self.ctx.type_ctx.get_type(tid))
            .is_some_and(|shape| matches!(shape, TypeShape::Enum(_)));
        let suppress_opaque_recursion =
            !is_captured && self.ctx.type_ctx.is_recursive(def_id) && !ref_returns_enum;

        let nav = nav_override.unwrap_or(Nav::Stay);

        // Call instructions don't have pre_effects, so emit epsilon if needed
        let call_entry = if needs_scope {
            // Captured ref returning struct: Obj → Call → EndObj → Set → exit
            // The Obj creates an isolated scope for the definition's internal captures.
            let set_step = self.emit_effects_epsilon(exit, capture.post, CaptureEffects::default());
            let endobj_step = self.emit_endobj_step(set_step);
            let call_label = self.emit_call(nav, field_override, endobj_step, target);
            self.emit_obj_step(call_label)
        } else if is_captured {
            // Captured ref returning scalar: Call → Set → exit
            let return_addr =
                self.emit_effects_epsilon(exit, capture.post, CaptureEffects::default());
            self.emit_call(nav, field_override, return_addr, target)
        } else if suppress_opaque_recursion {
            // Uncaptured opaque recursion: SuppressBegin → Call → SuppressEnd → exit.
            // The recursion still matches structurally but contributes no effects,
            // matching its inferred Void type.
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

        // Wrap with pre-effects epsilon (e.g., Enum for tagged alternations)
        self.emit_effects_epsilon(call_entry, capture.pre, CaptureEffects::default())
    }

    /// Compile a field constraint with capture effects (passed to inner pattern).
    pub(super) fn compile_field_inner(
        &mut self,
        field: &ast::FieldExpr,
        exit: Label,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        let Some(value) = field.value() else {
            return exit;
        };

        let node_field = self.resolve_field(field);

        // Special case: if value is a reference, pass field and capture to Call instruction
        if let Expr::Ref(r) = &value {
            return self.compile_ref_inner(r, exit, nav_override, node_field, capture);
        }

        // Check if value is a complex pattern (alternation, sequence, quantified)
        // that will produce an epsilon branch and thus need a wrapper instruction.
        // For these patterns, we must:
        // 1. Compile the value WITHOUT navigation (branches use Stay)
        // 2. Create a wrapper WITH navigation AND field check
        // This ensures the field is checked AFTER navigating to the child node.
        let needs_wrapper = node_field.is_some()
            && matches!(
                &value,
                Expr::AltExpr(_) | Expr::SeqExpr(_) | Expr::QuantifiedExpr(_)
            );

        if needs_wrapper {
            // Compile value WITHOUT navigation - wrapper will handle it
            let value_entry = self.compile_expr_inner(&value, exit, None, capture);

            let entry = self.fresh_label();
            self.instructions.push(
                MatchIR::epsilon(entry, value_entry)
                    .nav(nav_override.unwrap_or(Nav::Stay))
                    .node_field(node_field)
                    .into(),
            );
            return entry;
        }

        // Simple pattern: compile with navigation, merge field afterward
        let value_entry = self.compile_expr_inner(&value, exit, nav_override, capture);

        // If we have a field constraint, try to merge it into the value's instruction
        if let Some(field_id) = node_field {
            // Try to find and merge with the instruction we just created
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

            // Fallback: wrap with field-checking Match for patterns that couldn't merge.
            // Use Stay since value was already compiled with navigation.
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
    /// ordinary capture path ([`compile_expr_inner`](Self::compile_expr_inner)) and
    /// the navigating-first-child skippable path
    /// ([`compile_skippable_with_exits`](Self::compile_skippable_with_exits)) both
    /// route here, so a mechanism can never be handled by one and dropped by the
    /// other (the drift behind #470 and the suppressive `@_` panic).
    ///
    /// Capture effects land on the innermost match / scope-close instruction:
    /// - Node:   inner_pattern[Node/Text, Set] → exit
    /// - Struct: Obj → inner[…] → EndObj+capture → exit
    /// - Array:  Arr → quantifier (with Push) → EndArr+capture → exit
    /// - Ref:    Call → Set epsilon → exit
    /// - Suppressive: SuppressBegin → inner → SuppressEnd → outer_effects → exit
    pub(super) fn compile_captured(
        &mut self,
        cap: &ast::CapturedExpr,
        inner_opt: Option<Expr>,
        nav_override: Option<Nav>,
        outer_capture: CaptureEffects,
        exits: CaptureExits,
    ) -> Label {
        // Suppressive captures wrap their inner in SuppressBegin/SuppressEnd and emit
        // no value, whatever the inner's mechanism — so this comes before the
        // mechanism dispatch (and before building any capture effects).
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
        let mechanism = inner_opt
            .as_ref()
            .map(|inner| capture_mechanism(inner, self.ctx.type_ctx, self.ctx.interner));

        // [Node/Text] (only for the Node mechanism) + Set.
        let capture_effects = self.build_capture_effects(cap, mechanism);

        // Bare capture (`@x` with no inner): emit effects at the current position.
        let (Some(inner), Some(mechanism)) = (inner_opt, mechanism) else {
            return self.emit_effects_epsilon(exits.match_exit(), capture_effects, outer_capture);
        };

        match mechanism {
            // Array: Arr → quantifier (with Push) → EndArr+capture → exit(s).
            CaptureMechanism::Array => self.compile_array_capture(
                &inner,
                nav_override,
                capture_effects,
                outer_capture,
                cap.has_string_annotation(),
                exits,
            ),

            // Struct scope: Obj → inner → EndObj+capture → exit(s) (also empty `{}`).
            // Without the wrapper the Set lands on the raw inner node and both the
            // struct scope and the inner Sets are lost (#470).
            CaptureMechanism::StructScope => {
                let scope_type_id = self
                    .ctx
                    .type_ctx
                    .get_term_info(&inner)
                    .and_then(|info| info.flow.type_id());
                self.compile_struct_capture(
                    &inner,
                    nav_override,
                    scope_type_id,
                    capture_effects,
                    outer_capture,
                    exits,
                )
            }

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
                        match_exit,
                        skip_exit,
                        nav_override,
                        combined,
                    )
                }
                CaptureExits::Single(exit) => self.compile_passthrough_capture(
                    mechanism,
                    &inner,
                    exit,
                    nav_override,
                    capture_effects,
                    outer_capture,
                ),
            },
        }
    }

    /// Single-exit lowering for the pass-through mechanisms (Node/Ref/SetAfter):
    /// the captured value is produced by the inner itself, so the capture emits no
    /// scope — only a trailing `Set` (plus the `Node`/`Text` for a plain node).
    fn compile_passthrough_capture(
        &mut self,
        mechanism: CaptureMechanism,
        inner: &Expr,
        exit: Label,
        nav_override: Option<Nav>,
        capture_effects: Vec<EffectIR>,
        outer_capture: CaptureEffects,
    ) -> Label {
        match mechanism {
            // Set-after: the inner leaves the value pending (tagged alternation or a
            // named node forwarding a structured child). Emit the inner, then a
            // trailing Set; no Node, no wrapper.
            CaptureMechanism::SetAfter => {
                let CaptureEffects { pre, post } = outer_capture;
                let set_step = self.emit_effects_epsilon(
                    exit,
                    capture_effects,
                    CaptureEffects::new_post(post),
                );
                let inner_entry = self.compile_expr_inner(
                    inner,
                    set_step,
                    nav_override,
                    CaptureEffects::default(),
                );
                // The enclosing variant's `Enum`-open (in `pre`) must run before the
                // inner produces its pending value; routing it through the trailing
                // `Set` step would drop it and unbalance the scope.
                self.wrap_entry_pre(inner_entry, pre)
            }

            // Ref: hand the capture to the call site, which wraps Call/Return (and
            // Obj/EndObj for struct-returning definitions) to isolate the
            // definition's internal captures before the Set.
            CaptureMechanism::Ref => {
                let combined = outer_capture.with_post_values(capture_effects);
                self.compile_expr_inner(inner, exit, nav_override, combined)
            }

            // Node: capture the matched node. Bubbling children, if any, set into the
            // current scope alongside the capture.
            CaptureMechanism::Node => {
                let inner_is_bubble = self
                    .ctx
                    .type_ctx
                    .get_term_info(inner)
                    .is_some_and(|info| info.flow.is_bubble());
                if inner_is_bubble {
                    self.compile_bubble_with_node_capture(
                        inner,
                        exit,
                        nav_override,
                        None,
                        capture_effects,
                        outer_capture,
                    )
                } else {
                    let combined = outer_capture.with_post_values(capture_effects);
                    self.compile_expr_inner(inner, exit, nav_override, combined)
                }
            }

            CaptureMechanism::Array | CaptureMechanism::StructScope => {
                unreachable!("scope mechanisms are handled by compile_captured")
            }
        }
    }

    /// Compile a suppressive capture (`@_`/`@_name`): wrap the inner in
    /// SuppressBegin/SuppressEnd and emit no value. The suppress region brackets
    /// whatever the inner emits (its own `Set`s, a skipped optional's nulls) and
    /// discards it at runtime, matching the `void` the type system infers.
    ///
    /// With `Split` exits the inner's match/skip paths route to two SuppressEnd
    /// steps. `outer.pre` (e.g. a tagged variant's `Enum`-open) runs before
    /// SuppressBegin in the enclosing scope, so the tag is produced and the later
    /// `EndEnum` matches.
    fn compile_suppressive(
        &mut self,
        inner: Option<&Expr>,
        nav_override: Option<Nav>,
        outer_capture: CaptureEffects,
        exits: CaptureExits,
    ) -> Label {
        let CaptureEffects { pre, post } = outer_capture;

        let Some(inner) = inner else {
            // Bare `@_` with no inner: just pass through outer effects. A bare
            // capture never skips, so the match exit is the only continuation.
            let exit = exits.match_exit();
            if pre.is_empty() && post.is_empty() {
                return exit;
            }
            let entry = self.emit_effects_epsilon(exit, vec![], CaptureEffects::new_post(post));
            return self.wrap_entry_pre(entry, pre);
        };

        // SuppressEnd + outer post (e.g. a tagged variant's EndEnum) closes each exit;
        // the inner is compiled with NO capture effects.
        let inner_entry = match exits {
            CaptureExits::Single(exit) => {
                let end_label = self.emit_effects_epsilon(
                    exit,
                    vec![EffectIR::suppress_end()],
                    CaptureEffects::new_post(post),
                );
                self.compile_expr_inner(inner, end_label, nav_override, CaptureEffects::default())
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
                    end_match,
                    end_skip,
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

    /// Resolve an anonymous node's literal text to its node type constraint.
    ///
    /// Returns `NodeTypeIR::Anonymous` with the type ID.
    pub(super) fn resolve_anonymous_node_type(&mut self, text: &str) -> NodeTypeIR {
        if let Some(ids) = self.ctx.node_types {
            // Linked mode: resolve to NodeTypeId from grammar
            let Some(sym) = self.ctx.interner.get(text) else {
                return NodeTypeIR::Anonymous(None);
            };
            ids.get(&NodeType::Anonymous(sym))
                .and_then(|id| NonZeroU16::new(id.get()))
                .map_or(NodeTypeIR::Anonymous(None), |id| {
                    NodeTypeIR::Anonymous(Some(id))
                })
        } else {
            // Unlinked mode: store StringId referencing the literal text
            let string_id = self.ctx.strings.borrow_mut().intern_str(text);
            NodeTypeIR::Anonymous(Some(string_id.0))
        }
    }

    /// Resolve a NamedNode to its node type constraint.
    ///
    /// Returns `NodeTypeIR::Named` with:
    /// - `None` for wildcard `(_)` (any named node)
    /// - `Some(id)` for specific types like `(identifier)`
    pub(super) fn resolve_node_type(&mut self, node: &ast::NamedNode) -> NodeTypeIR {
        // For wildcard (_), return Named(None) for "any named node"
        if node.is_any() {
            return NodeTypeIR::Named(None);
        }

        let Some(type_token) = node.node_type() else {
            // No type specified - treat as any named
            return NodeTypeIR::Named(None);
        };
        let type_name = type_token.text();

        if let Some(ids) = self.ctx.node_types {
            // Linked mode: resolve to NodeTypeId from grammar
            let Some(sym) = self.ctx.interner.get(type_name) else {
                return NodeTypeIR::Named(None);
            };
            ids.get(&NodeType::Named(sym))
                .and_then(|id| NonZeroU16::new(id.get()))
                .map_or(NodeTypeIR::Named(None), |id| NodeTypeIR::Named(Some(id)))
        } else {
            // Unlinked mode: store StringId referencing the type name
            let string_id = self.ctx.strings.borrow_mut().intern_str(type_name);
            NodeTypeIR::Named(Some(string_id.0))
        }
    }

    /// Resolve a field expression to its field ID.
    ///
    /// In linked mode, returns the grammar NodeFieldId.
    /// In unlinked mode, returns the StringId of the field name.
    pub(super) fn resolve_field(&mut self, field: &ast::FieldExpr) -> Option<NonZeroU16> {
        let name_token = field.name()?;
        let field_name = name_token.text();
        self.resolve_field_by_name(field_name)
    }

    /// Resolve a field name to its field ID.
    ///
    /// In linked mode, returns the grammar NodeFieldId.
    /// In unlinked mode, returns the StringId of the field name.
    pub(super) fn resolve_field_by_name(&mut self, field_name: &str) -> Option<NonZeroU16> {
        if let Some(ids) = self.ctx.node_fields {
            // Linked mode: an unknown field is left unconstrained.
            self.ctx
                .interner
                .get(field_name)
                .and_then(|sym| ids.get(&sym))
                .and_then(|id| NonZeroU16::new(id.get()))
        } else {
            // Unlinked mode: store StringId referencing the field name
            let string_id = self.ctx.strings.borrow_mut().intern_str(field_name);
            Some(string_id.0)
        }
    }

    /// Collect negated fields from a NamedNode.
    pub(super) fn collect_neg_fields(&mut self, node: &ast::NamedNode) -> Vec<u16> {
        node.as_cst()
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
    pub(super) fn compile_predicate(&mut self, node: &ast::NamedNode) -> Option<PredicateIR> {
        let pred = node.predicate()?;
        let op = pred.operator()?;

        // Try string value first
        if let Some(str_token) = pred.string_value() {
            let string_id = self.ctx.strings.borrow_mut().intern_str(str_token.text());
            return Some(PredicateIR::string(op, string_id));
        }

        // Try regex value
        if let Some(regex) = pred.regex() {
            // Get pattern text by stripping `/` delimiters from CST text
            let text: String = regex.as_cst().text().into();
            let without_prefix = text.strip_prefix('/').unwrap_or(&text);
            let pattern = without_prefix.strip_suffix('/').unwrap_or(without_prefix);
            let string_id = self.ctx.strings.borrow_mut().intern_str(pattern);
            return Some(PredicateIR::regex(op, string_id));
        }

        None
    }
}
