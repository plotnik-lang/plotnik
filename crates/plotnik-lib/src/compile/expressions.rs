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
use crate::bytecode::ir::{EffectIR, Instruction, Label, MatchIR};
use crate::bytecode::{EffectOpcode, Nav};
use crate::parser::ast::{self, Expr};

use super::Compiler;
use super::capture::CaptureEffects;
use super::navigation::{
    check_trailing_anchor, inner_creates_scope, is_skippable_quantifier, is_star_or_plus_quantifier,
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
            self.instructions.push(Instruction::Match(MatchIR {
                label: entry,
                nav,
                node_type,
                node_field: None,
                pre_effects: vec![],
                neg_fields,
                post_effects: capture.post,
                successors: vec![exit],
            }));
            return entry;
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

        // Check if first item is skippable - its skip path should bypass the Up.
        // When a zero-match quantifier (? or *) is the first child with Down navigation,
        // the skip path never descends, so executing Up would ascend too far.
        let first_is_skippable = items
            .first()
            .and_then(|i| i.as_expr())
            .is_some_and(is_skippable_quantifier);

        // With items: nav → items → Up → exit
        // If first item is skippable: skip path → exit (bypass Up), match path → Up → exit
        let up_label = self.fresh_label();
        let skip_exit = first_is_skippable.then_some(exit);
        let items_entry = self.compile_seq_items_inner(
            &items,
            up_label,
            true,
            None,
            CaptureEffects::default(),
            skip_exit,
        );

        self.instructions.push(Instruction::Match(MatchIR {
            label: up_label,
            nav: up_nav,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![exit],
        }));

        // Emit entry instruction into the node (capture effects go here at match time)
        self.instructions.push(Instruction::Match(MatchIR {
            label: entry,
            nav,
            node_type,
            node_field: None,
            pre_effects: vec![],
            neg_fields,
            post_effects: capture.post,
            successors: vec![items_entry],
        }));

        entry
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

        // Extract literal value (None for wildcard `_`)
        let node_type = node.value().and_then(|token| {
            let text = token.text();
            self.resolve_anonymous_node_type(text)
        });

        self.instructions.push(Instruction::Match(MatchIR {
            label: entry,
            nav,
            node_type,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: capture.post,
            successors: vec![exit],
        }));

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

        if needs_scope {
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
        }
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
            self.instructions.push(Instruction::Match(MatchIR {
                label: entry,
                nav: nav_override.unwrap_or(Nav::Stay),
                node_type: None,
                node_field,
                pre_effects: vec![],
                neg_fields: vec![],
                post_effects: vec![],
                successors: vec![value_entry],
            }));
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
                && let Instruction::Match(m) = instr
                && m.node_field.is_none()
            {
                m.node_field = Some(field_id);
                return value_entry;
            }

            // Fallback: wrap with field-checking Match for patterns that couldn't merge.
            // Use Stay since value was already compiled with navigation.
            let entry = self.fresh_label();
            self.instructions.push(Instruction::Match(MatchIR {
                label: entry,
                nav: Nav::Stay,
                node_type: None,
                node_field: Some(field_id),
                pre_effects: vec![],
                neg_fields: vec![],
                post_effects: vec![],
                successors: vec![value_entry],
            }));
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
        if inner_is_bubble {
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
            );
        }

        // Scalar: capture effects go directly on the match instruction
        let mut combined = capture_effects;
        combined.extend(outer_capture.post);
        self.compile_expr_inner(
            &inner,
            exit,
            nav_override,
            CaptureEffects { post: combined },
        )
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
        let suppress_end = vec![EffectIR::simple(EffectOpcode::SuppressEnd, 0)];
        let end_label = self.emit_effects_epsilon(exit, suppress_end, outer_capture);

        // Compile inner → end_label (inner gets NO capture effects)
        let inner_entry =
            self.compile_expr_inner(&inner, end_label, nav_override, CaptureEffects::default());

        // SuppressBegin → inner_entry
        let suppress_begin = vec![EffectIR::simple(EffectOpcode::SuppressBegin, 0)];
        self.emit_effects_epsilon(inner_entry, suppress_begin, CaptureEffects::default())
    }

    /// Resolve an anonymous node's literal text to its node type ID.
    ///
    /// In linked mode, returns the grammar NodeTypeId for the literal.
    /// In unlinked mode, returns the StringId of the literal text.
    pub(super) fn resolve_anonymous_node_type(&mut self, text: &str) -> Option<NonZeroU16> {
        if let Some(ids) = self.node_type_ids {
            // Linked mode: resolve to NodeTypeId from grammar
            for (&sym, &id) in ids {
                if self.interner.resolve(sym) == text {
                    return NonZeroU16::new(id.get());
                }
            }
            // If not found in grammar, treat as no constraint
            None
        } else {
            // Unlinked mode: store StringId referencing the literal text
            let string_id = self.strings.intern_str(text);
            Some(string_id.0)
        }
    }

    /// Resolve a NamedNode to its node type ID.
    ///
    /// In linked mode, returns the grammar NodeTypeId.
    /// In unlinked mode, returns the StringId of the type name.
    pub(super) fn resolve_node_type(&mut self, node: &ast::NamedNode) -> Option<NonZeroU16> {
        // For wildcard (_), no constraint
        if node.is_any() {
            return None;
        }

        let type_token = node.node_type()?;
        let type_name = type_token.text();

        if let Some(ids) = self.node_type_ids {
            // Linked mode: resolve to NodeTypeId from grammar
            for (&sym, &id) in ids {
                if self.interner.resolve(sym) == type_name {
                    return NonZeroU16::new(id.get());
                }
            }
            // If not found in grammar, treat as no constraint (linked mode)
            None
        } else {
            // Unlinked mode: store StringId referencing the type name
            let string_id = self.strings.intern_str(type_name);
            Some(string_id.0)
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
