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
use crate::bytecode::Nav;
use crate::bytecode::{EffectIR, InstructionIR, Label, MatchIR, NodeTypeIR};
use crate::parser::ast::{self, Expr};

use super::Compiler;
use super::capture::CaptureEffects;
use super::navigation::{
    check_trailing_anchor, inner_creates_scope, is_star_or_plus_quantifier, is_truly_empty_scope,
};

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

        // Collect items and negated fields
        let items: Vec<_> = node.items().collect();
        let neg_fields = self.collect_neg_fields(node);

        // If no items, just match and exit (capture effects go here)
        if items.is_empty() {
            return self.emit_match_with_cascade(
                MatchIR::epsilon(entry, exit)
                    .nav(nav)
                    .node_type(node_type)
                    .neg_fields(neg_fields)
                    .pre_effects(capture.pre)
                    .post_effects(capture.post),
            );
        }

        // Determine Up navigation based on trailing anchor
        let (has_trailing_anchor, trailing_strictness) = check_trailing_anchor(&items);

        // Emit Up instruction with appropriate strictness
        let up_nav = if has_trailing_anchor {
            if trailing_strictness {
                Nav::UpExact(1)
            } else {
                Nav::UpSkipTrivia(1)
            }
        } else {
            Nav::Up(1)
        };

        // Trailing anchor requires skip-retry pattern for backtracking.
        // When the anchor check fails (matched node is not last), we need to
        // retry with the next sibling until we find one that IS last.
        if has_trailing_anchor {
            return self.compile_named_node_with_trailing_anchor(
                entry, exit, nav, node_type, neg_fields, &items, up_nav, capture,
            );
        }

        // Split capture.post: Node/Text effects (and their Set) go on entry (need matched_node
        // right after match), other effects go on final_exit (after children processing).
        // Node/Text capture effects use matched_node which is only valid immediately after the match,
        // before descending into children (which may clobber matched_node via backtracking).
        use crate::bytecode::EffectOpcode;

        // Find Node/Text effects and their following Set effects (they come in pairs: Node Set)
        let mut entry_effects = Vec::new();
        let mut exit_effects = Vec::new();
        let mut iter = capture.post.into_iter().peekable();
        while let Some(eff) = iter.next() {
            if matches!(eff.opcode, EffectOpcode::Node | EffectOpcode::Text) {
                entry_effects.push(eff);
                // Take the following Set if present (Node/Text are always followed by Set)
                if iter.peek().is_some_and(|e| e.opcode == EffectOpcode::Set) {
                    entry_effects.push(iter.next().unwrap());
                }
            } else {
                exit_effects.push(eff);
            }
        }

        // With items: nav[entry_effects] → items → Up → [exit_effects] → exit
        let final_exit = self.emit_post_effects_exit(exit, exit_effects);

        let up_label = self.fresh_label();
        let items_entry = self.compile_seq_items_inner(
            &items,
            up_label,
            true,
            None,
            CaptureEffects::default(),
            None, // No skip_exit bypass - all paths need Up
        );

        self.instructions
            .push(MatchIR::epsilon(up_label, final_exit).nav(up_nav).into());

        // Emit entry instruction with node_effects on post (executes right after match)
        self.emit_match_with_cascade(
            MatchIR::epsilon(entry, items_entry)
                .nav(nav)
                .node_type(node_type)
                .neg_fields(neg_fields)
                .pre_effects(capture.pre)
                .post_effects(entry_effects),
        );

        entry
    }

    /// Compile a named node with trailing anchor using skip-retry pattern.
    ///
    /// Structure:
    /// ```text
    /// entry: Match(nav, node_type) → down_wildcard
    /// down_wildcard: Match(Down, wildcard) → try
    /// try: epsilon → [body, retry_nav]
    /// body: items (StayExact) → up_check
    /// up_check: Match(up_nav, None) → exit
    /// retry_nav: Match(Next, wildcard) → try
    /// ```
    ///
    /// When items match but the trailing anchor check fails, we backtrack to `try`,
    /// which falls through to `retry_nav`, advances to next sibling, and retries.
    /// Only when siblings are exhausted does backtracking propagate to the caller.
    #[allow(clippy::too_many_arguments)]
    fn compile_named_node_with_trailing_anchor(
        &mut self,
        entry: Label,
        exit: Label,
        nav: Nav,
        node_type: NodeTypeIR,
        neg_fields: Vec<u16>,
        items: &[ast::SeqItem],
        up_nav: Nav,
        capture: CaptureEffects,
    ) -> Label {
        let final_exit = self.emit_post_effects_exit(exit, capture.post);

        // up_check: Match(up_nav) → final_exit
        let up_check = self.fresh_label();
        self.instructions
            .push(MatchIR::epsilon(up_check, final_exit).nav(up_nav).into());

        // body: items with StayExact navigation → up_check
        // Items are compiled with StayExact because the skip-retry loop handles
        // advancement; the body should match at the current position only.
        let body = self.compile_seq_items_inner(
            items,
            up_check,
            true,
            Some(Nav::StayExact), // First item uses StayExact (we're already at position)
            CaptureEffects::default(),
            None,
        );

        // Build skip-retry structure:
        // try: epsilon → [body, retry_nav]
        // retry_nav: Match(Next, wildcard) → try
        let try_label = self.fresh_label();
        let retry_nav = self.fresh_label();

        // retry_nav: advance to next sibling and loop back
        self.emit_wildcard_nav(retry_nav, Nav::Next, try_label);

        // try: branch to body (prefer) or retry (fallback)
        // Greedy: try body first, then retry on failure
        self.emit_branch_epsilon_at(try_label, body, retry_nav, true);

        // down_wildcard: navigate to first child → try
        let down_wildcard = self.fresh_label();
        self.emit_wildcard_nav(down_wildcard, Nav::Down, try_label);

        // entry: match parent node → down_wildcard (only pre_effects here)
        self.emit_match_with_cascade(
            MatchIR::epsilon(entry, down_wildcard)
                .nav(nav)
                .node_type(node_type)
                .neg_fields(neg_fields)
                .pre_effects(capture.pre),
        );

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

        self.emit_match_with_cascade(
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

        let Some(def_id) = self.type_ctx.get_def_id(self.interner, name) else {
            return exit;
        };

        let Some(&target) = self.def_entries.get(&def_id) else {
            return exit;
        };

        // Check if the called definition returns a struct (needs scope isolation when captured)
        let def_type_id = self.type_ctx.get_def_type(def_id);
        let ref_returns_struct = def_type_id
            .and_then(|tid| self.type_ctx.get_type(tid))
            .is_some_and(|shape| matches!(shape, TypeShape::Struct(_)));

        // Determine if this is a captured ref that needs scope isolation
        let is_captured = !capture.post.is_empty();
        let needs_scope = is_captured && ref_returns_struct;

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

    /// Compile a captured expression with capture effects from outer layers.
    ///
    /// Capture effects are placed on the innermost match instruction:
    /// - Scalar: inner_pattern[Node/Text, Set] → exit
    /// - Struct: Obj epsilon → inner_pattern[Node/Text, Set] → EndObj epsilon → exit
    /// - Array:  Arr epsilon → quantifier (with Push on body) → EndArr+Set epsilon → exit
    /// - Ref:    Call → Set epsilon → exit (structured result needs epsilon)
    /// - Suppressive: SuppressBegin → inner → SuppressEnd → outer_effects → exit
    pub(super) fn compile_captured_inner(
        &mut self,
        cap: &ast::CapturedExpr,
        exit: Label,
        nav_override: Option<Nav>,
        outer_capture: CaptureEffects,
    ) -> Label {
        // Handle suppressive captures: wrap inner with SuppressBegin/End
        if cap.is_suppressive() {
            return self.compile_suppressive_capture(cap, exit, nav_override, outer_capture);
        }

        let inner = cap.inner();
        let inner_info = inner.as_ref().and_then(|i| self.type_ctx.get_term_info(i));
        let inner_is_bubble = inner_info
            .as_ref()
            .is_some_and(|info| info.flow.is_bubble());
        let inner_creates_scope = inner.as_ref().is_some_and(inner_creates_scope);

        // Scope type for inner compilation:
        // - Scope-creating expressions (sequences, alternations) push their own scope
        //   so inner captures reference the sequence's struct type.
        // - Non-scope-creating bubbles (named nodes) don't push a scope - their fields
        //   bubble up to the parent scope, so inner captures should reference that.
        let scope_type_id = if inner_creates_scope {
            inner_info.as_ref().and_then(|info| info.flow.type_id())
        } else {
            None // Bubbles don't create new scopes
        };

        // Build capture effects: [Node/Text] followed by Set(member_ref)
        let capture_effects = self.build_capture_effects(cap, inner.as_ref());

        // Bare capture: just emit effects at current position
        let Some(inner) = inner else {
            return self.emit_effects_epsilon(exit, capture_effects, outer_capture);
        };

        // Struct scope: Obj → inner → EndObj+capture → exit
        // Also handle truly empty scopes (e.g., `{ } @x` produces empty struct)
        let inner_is_truly_empty_scope = is_truly_empty_scope(&inner);
        let needs_struct_scope = inner_is_bubble || inner_is_truly_empty_scope;

        if needs_struct_scope {
            return if inner_creates_scope {
                // Sequence/alternation: capture effects after EndObj (value is the struct)
                self.compile_struct_scope(
                    &inner,
                    exit,
                    nav_override,
                    scope_type_id,
                    capture_effects,
                    outer_capture,
                )
            } else {
                // Node with bubbles: scope wrapper for inner captures, but capture on inner match
                self.compile_bubble_with_node_capture(
                    &inner,
                    exit,
                    nav_override,
                    scope_type_id,
                    capture_effects,
                    outer_capture,
                )
            };
        }

        // Handle scope-creating scalar expressions (tagged enums)
        // Enum produces its own value via EndEnum - capture effects go AFTER, not inside
        let inner_is_scope_creating_scalar = !inner_is_bubble
            && inner_creates_scope
            && inner_info
                .as_ref()
                .and_then(|info| info.flow.type_id())
                .and_then(|id| self.type_ctx.get_type(id))
                .is_some_and(|shape| matches!(shape, TypeShape::Enum(_)));

        if inner_is_scope_creating_scalar {
            let set_step = self.emit_effects_epsilon(exit, capture_effects, outer_capture);
            return self.compile_expr_inner(
                &inner,
                set_step,
                nav_override,
                CaptureEffects::default(),
            );
        }

        // Array: Arr → quantifier (with Push) → EndArr+capture → exit
        // Check if inner is a * or + quantifier - these produce arrays regardless of arity
        let inner_is_array = is_star_or_plus_quantifier(Some(&inner));

        if inner_is_array {
            return self.compile_array_scope(
                &inner,
                exit,
                nav_override,
                capture_effects,
                outer_capture,
                cap.has_string_annotation(),
            );
        }

        // Scalar: capture effects go directly on the match instruction
        let combined = outer_capture.with_post_values(capture_effects);
        self.compile_expr_inner(&inner, exit, nav_override, combined)
    }

    /// Compile a suppressive capture (@_ or @_name).
    ///
    /// Suppressive captures match structurally but don't emit effects.
    /// Flow: SuppressBegin → inner → SuppressEnd → outer_effects → exit
    fn compile_suppressive_capture(
        &mut self,
        cap: &ast::CapturedExpr,
        exit: Label,
        nav_override: Option<Nav>,
        outer_capture: CaptureEffects,
    ) -> Label {
        let Some(inner) = cap.inner() else {
            // Bare @_ with no inner - just pass through outer effects
            if outer_capture.post.is_empty() {
                return exit;
            }
            return self.emit_effects_epsilon(exit, vec![], outer_capture);
        };

        // SuppressEnd + outer capture effects → exit
        let suppress_end = vec![EffectIR::suppress_end()];
        let end_label = self.emit_effects_epsilon(exit, suppress_end, outer_capture);

        // Compile inner → end_label (inner gets NO capture effects)
        let inner_entry =
            self.compile_expr_inner(&inner, end_label, nav_override, CaptureEffects::default());

        // SuppressBegin → inner_entry
        let suppress_begin = vec![EffectIR::suppress_begin()];
        self.emit_effects_epsilon(inner_entry, suppress_begin, CaptureEffects::default())
    }

    /// Resolve an anonymous node's literal text to its node type constraint.
    ///
    /// Returns `NodeTypeIR::Anonymous` with the type ID.
    pub(super) fn resolve_anonymous_node_type(&mut self, text: &str) -> NodeTypeIR {
        if let Some(ids) = self.node_type_ids {
            // Linked mode: resolve to NodeTypeId from grammar
            for (&sym, &id) in ids {
                if self.interner.resolve(sym) == text {
                    return NodeTypeIR::Anonymous(NonZeroU16::new(id.get()));
                }
            }
            // If not found in grammar, treat as anonymous wildcard
            NodeTypeIR::Anonymous(None)
        } else {
            // Unlinked mode: store StringId referencing the literal text
            let string_id = self.strings.intern_str(text);
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

        if let Some(ids) = self.node_type_ids {
            // Linked mode: resolve to NodeTypeId from grammar
            for (&sym, &id) in ids {
                if self.interner.resolve(sym) == type_name {
                    return NodeTypeIR::Named(NonZeroU16::new(id.get()));
                }
            }
            // If not found in grammar, treat as any named (linked mode)
            NodeTypeIR::Named(None)
        } else {
            // Unlinked mode: store StringId referencing the type name
            let string_id = self.strings.intern_str(type_name);
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
        if let Some(ids) = self.node_field_ids {
            // Linked mode: resolve to NodeFieldId from grammar
            for (&sym, &id) in ids {
                if self.interner.resolve(sym) == field_name {
                    return NonZeroU16::new(id.get());
                }
            }
            // If not found in grammar, treat as no constraint (linked mode)
            None
        } else {
            // Unlinked mode: store StringId referencing the field name
            let string_id = self.strings.intern_str(field_name);
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
}
