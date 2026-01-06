//! Scope management for structured captures.
//!
//! Handles Obj/EndObj and Arr/EndArr wrapper emission for struct and array captures.

use std::num::NonZeroU16;

use crate::analyze::type_check::TypeId;
use crate::bytecode::{CallIR, EffectIR, Label, MatchIR, MemberRef};
use crate::bytecode::{EffectOpcode, Nav};
use crate::parser::Expr;

use super::Compiler;
use super::capture::CaptureEffects;

/// Struct scope for tracking captures in nested contexts.
/// Each scope represents a struct type whose fields can receive captures.
#[derive(Clone, Copy, Debug)]
pub struct StructScope(pub TypeId);

impl Compiler<'_> {
    /// Execute with optional scope - avoids repeated if-let pattern.
    pub(super) fn compile_with_optional_scope<T>(
        &mut self,
        type_id: Option<TypeId>,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        if let Some(type_id) = type_id {
            self.with_scope(type_id, f)
        } else {
            f(self)
        }
    }

    /// Execute a closure with a scope pushed, automatically popping afterward.
    pub(super) fn with_scope<T>(&mut self, type_id: TypeId, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scope_stack.push(StructScope(type_id));
        let result = f(self);
        self.scope_stack.pop();
        result
    }

    /// Look up a capture name in a type, returning a deferred member reference.
    ///
    /// Uses (struct_type, relative_index) for deferred resolution.
    /// Member deduplication for call-site scoping will be added later.
    pub(super) fn lookup_member(&self, capture_name: &str, type_id: TypeId) -> Option<MemberRef> {
        let fields = self.type_ctx.get_struct_fields(type_id)?;
        for (relative_index, (&field_sym, _)) in fields.iter().enumerate() {
            if self.interner.resolve(field_sym) == capture_name {
                return Some(MemberRef::deferred_by_index(type_id, relative_index as u16));
            }
        }
        None
    }

    /// Look up a capture name in the current scope stack.
    pub(super) fn lookup_member_in_scope(&self, capture_name: &str) -> Option<MemberRef> {
        let StructScope(type_id) = *self.scope_stack.last()?;
        self.lookup_member(capture_name, type_id)
    }

    /// Compile struct scope: Obj → inner → EndObj+capture → exit
    pub(super) fn compile_struct_scope(
        &mut self,
        inner: &Expr,
        exit: Label,
        nav_override: Option<Nav>,
        scope_type_id: Option<TypeId>,
        capture_effects: Vec<EffectIR>,
        outer_capture: CaptureEffects,
    ) -> Label {
        let endobj_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(endobj_step, exit)
                .post_effect(EffectIR::end_obj())
                .post_effects(capture_effects)
                .post_effects(outer_capture.post)
                .into(),
        );

        let inner_entry = self.compile_with_optional_scope(scope_type_id, |this| {
            this.compile_expr_with_nav(inner, endobj_step, nav_override)
        });

        let obj_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(obj_step, inner_entry)
                .pre_effect(EffectIR::start_obj())
                .into(),
        );

        obj_step
    }

    /// Compile bubble with node capture: inner[capture] → exit (with optional outer effects)
    ///
    /// Used when a named node contains bubbling captures but the capture itself
    /// should capture the node value (not a struct). The capture_effects go on
    /// the inner match instruction, and outer_capture effects are emitted after.
    ///
    /// Note: Previously this always wrapped in Obj/EndObj, but that was incorrect
    /// when scope_type_id is None. The inner captures use the current scope from
    /// the outer context (e.g., array row struct), so no new scope is needed.
    pub(super) fn compile_bubble_with_node_capture(
        &mut self,
        inner: &Expr,
        exit: Label,
        nav_override: Option<Nav>,
        scope_type_id: Option<TypeId>,
        capture_effects: Vec<EffectIR>,
        outer_capture: CaptureEffects,
    ) -> Label {
        // When scope_type_id is None, inner captures use the current scope
        // (no new Obj/EndObj scope needed - just compile with combined effects)
        if scope_type_id.is_none() {
            // If we have outer_capture effects (like Push), emit epsilon step for them
            let actual_exit = if outer_capture.post.is_empty() {
                exit
            } else {
                let outer_step = self.fresh_label();
                self.instructions.push(
                    MatchIR::epsilon(outer_step, exit)
                        .post_effects(outer_capture.post)
                        .into(),
                );
                outer_step
            };

            // Compile inner with capture_effects on the match instruction
            let inner_capture = CaptureEffects {
                pre: outer_capture.pre,
                post: capture_effects,
            };
            return self.compile_expr_inner(inner, actual_exit, nav_override, inner_capture);
        }

        // When scope_type_id is Some, we need Obj/EndObj to create the scope
        // EndObj step with ONLY outer_capture effects (like Push), NOT capture_effects
        let endobj_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(endobj_step, exit)
                .post_effect(EffectIR::end_obj())
                .post_effects(outer_capture.post)
                .into(),
        );

        // Compile inner WITH capture_effects on the match instruction
        // Note: pre effects don't propagate through Obj/EndObj scope wrapper
        let inner_capture = CaptureEffects {
            pre: vec![],
            post: capture_effects,
        };
        let inner_entry = self.with_scope(scope_type_id.unwrap(), |this| {
            this.compile_expr_inner(inner, endobj_step, nav_override, inner_capture)
        });

        let obj_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(obj_step, inner_entry)
                .pre_effect(EffectIR::start_obj())
                .into(),
        );

        obj_step
    }

    /// Compile array scope: Arr → quantifier (with Push) → EndArr+capture → exit
    ///
    /// `use_text_for_elements` indicates whether to use `Text` effect for array elements
    /// (true when the capture has `:: string` annotation).
    pub(super) fn compile_array_scope(
        &mut self,
        inner: &Expr,
        exit: Label,
        nav_override: Option<Nav>,
        capture_effects: Vec<EffectIR>,
        outer_capture: CaptureEffects,
        use_text_for_elements: bool,
    ) -> Label {
        let endarr_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(endarr_step, exit)
                .post_effect(EffectIR::end_arr())
                .post_effects(capture_effects)
                .post_effects(outer_capture.post)
                .into(),
        );

        let push_effects = CaptureEffects {
            pre: vec![],
            post: if self.quantifier_needs_node_for_push(inner) {
                // Use Text if the capture has `:: string` annotation, else Node
                let node_eff = if use_text_for_elements {
                    EffectIR::text()
                } else {
                    EffectIR::node()
                };
                vec![node_eff, EffectIR::push()]
            } else {
                vec![EffectIR::push()]
            },
        };
        let inner_entry = if let Expr::QuantifiedExpr(quant) = inner {
            self.compile_quantified_for_array(quant, endarr_step, nav_override, push_effects)
        } else {
            self.compile_expr_with_nav(inner, endarr_step, nav_override)
        };

        let arr_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(arr_step, inner_entry)
                .pre_effect(EffectIR::start_arr())
                .into(),
        );

        arr_step
    }

    /// Compile an expression with Obj/EndObj wrapping for array iteration.
    ///
    /// Used when inner is a scope-creating expression (sequence/alternation) with
    /// internal captures. Each iteration produces: Obj → inner → EndObj Push
    pub(super) fn compile_struct_for_array(
        &mut self,
        inner: &Expr,
        exit: Label,
        nav_override: Option<Nav>,
        row_type_id: Option<TypeId>,
    ) -> Label {
        // EndObj Push → exit
        let endobj_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(endobj_step, exit)
                .post_effect(EffectIR::end_obj())
                .post_effect(EffectIR::push())
                .into(),
        );

        // Compile inner with row scope (for Set effects to work)
        let inner_entry = self.compile_with_optional_scope(row_type_id, |this| {
            this.compile_expr_with_nav(inner, endobj_step, nav_override)
        });

        // Obj → inner_entry
        let obj_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(obj_step, inner_entry)
                .pre_effect(EffectIR::start_obj())
                .into(),
        );

        obj_step
    }

    /// Emit an EndArr epsilon step with the given effects.
    pub(super) fn emit_endarr_step(
        &mut self,
        capture_effects: &[EffectIR],
        outer_effects: &[EffectIR],
        exit: Label,
    ) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, exit)
                .post_effect(EffectIR::end_arr())
                .post_effects(capture_effects.iter().cloned())
                .post_effects(outer_effects.iter().cloned())
                .into(),
        );
        label
    }

    /// Emit an Obj epsilon step.
    pub(super) fn emit_obj_step(&mut self, successor: Label) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, successor)
                .pre_effect(EffectIR::start_obj())
                .into(),
        );
        label
    }

    /// Emit an EndObj epsilon step.
    pub(super) fn emit_endobj_step(&mut self, successor: Label) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, successor)
                .post_effect(EffectIR::end_obj())
                .into(),
        );
        label
    }

    /// Emit an Arr epsilon step.
    pub(super) fn emit_arr_step(&mut self, successor: Label) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, successor)
                .pre_effect(EffectIR::start_arr())
                .into(),
        );
        label
    }

    /// Emit a Call instruction.
    pub(super) fn emit_call(
        &mut self,
        nav: Nav,
        node_field: Option<NonZeroU16>,
        next: Label,
        target: Label,
    ) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            CallIR::new(label, target, next)
                .nav(nav)
                .node_field(node_field)
                .into(),
        );
        label
    }

    /// Emit an epsilon with combined effects.
    pub(super) fn emit_effects_epsilon(
        &mut self,
        exit: Label,
        effects: Vec<EffectIR>,
        outer: CaptureEffects,
    ) -> Label {
        let entry = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(entry, exit)
                .post_effects(effects)
                .post_effects(outer.post)
                .into(),
        );
        entry
    }

    /// Emit null effects for a skip path in optional/star quantifiers.
    ///
    /// When an optional/star pattern is skipped, any captures it would have set
    /// need to be explicitly nulled. This mirrors the null injection that
    /// alternations do for asymmetric branches.
    ///
    /// Returns the new exit label (with null effects) or the original exit if
    /// no null effects are needed.
    pub(super) fn emit_null_for_skip_path(
        &mut self,
        exit: Label,
        capture: &CaptureEffects,
    ) -> Label {
        // Collect Set effects - these are the fields that need nulling
        let null_effects: Vec<_> = capture
            .post
            .iter()
            .filter(|eff| eff.opcode == EffectOpcode::Set)
            .flat_map(|set_eff| [EffectIR::null(), set_eff.clone()])
            .collect();

        if null_effects.is_empty() {
            return exit;
        }

        let null_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(null_step, exit)
                .post_effects(null_effects)
                .into(),
        );
        null_step
    }

    /// Emit null effects for internal captures when skipping an optional/star pattern.
    ///
    /// Unlike `emit_null_for_skip_path` which handles captures passed as effects,
    /// this function handles captures defined INSIDE the expression (e.g., `{(x) @cap}?`).
    /// It collects all capture names from the expression and emits Null Set for each.
    pub(super) fn emit_null_for_internal_captures(&mut self, exit: Label, inner: &Expr) -> Label {
        let captures = Self::collect_captures(inner);
        if captures.is_empty() {
            return exit;
        }

        let mut null_effects = Vec::new();
        for name in captures {
            if let Some(member_ref) = self.lookup_member_in_scope(&name) {
                null_effects.push(EffectIR::null());
                null_effects.push(EffectIR::with_member(EffectOpcode::Set, member_ref));
            }
        }

        if null_effects.is_empty() {
            return exit;
        }

        self.emit_effects_epsilon(exit, null_effects, CaptureEffects::default())
    }

    /// Emit an epsilon transition (no node interaction).
    ///
    /// If there are more successors than fit in a single Match instruction,
    /// this creates a cascade of epsilon transitions to preserve NFA semantics.
    pub(super) fn emit_epsilon(&mut self, label: Label, successors: Vec<Label>) {
        use crate::bytecode::MAX_MATCH_PAYLOAD_SLOTS;

        if successors.len() <= MAX_MATCH_PAYLOAD_SLOTS {
            self.push_epsilon(label, successors);
            return;
        }

        // Split: first (MAX-1) successors + intermediate for rest.
        // This preserves priority order: VM tries s0, s1, ..., then intermediate.
        let split_at = MAX_MATCH_PAYLOAD_SLOTS - 1;
        let (first_batch, rest) = successors.split_at(split_at);

        let intermediate = self.fresh_label();
        self.emit_epsilon(intermediate, rest.to_vec());

        let mut batch = first_batch.to_vec();
        batch.push(intermediate);
        self.push_epsilon(label, batch);
    }

    fn push_epsilon(&mut self, label: Label, successors: Vec<Label>) {
        self.instructions
            .push(MatchIR::at(label).next_many(successors).into());
    }

    /// Emit a wildcard navigation step that accepts any node.
    ///
    /// Used for skip-retry logic in quantifiers: navigates to the next position
    /// and matches any node there. If navigation fails (no more siblings/children),
    /// the VM backtracks automatically.
    pub(super) fn emit_wildcard_nav(&mut self, label: Label, nav: Nav, successor: Label) {
        self.instructions
            .push(MatchIR::epsilon(label, successor).nav(nav).into());
    }

    /// Emit an epsilon branch preferring `prefer` when greedy, `other` when non-greedy.
    pub(super) fn emit_branch_epsilon(
        &mut self,
        prefer: Label,
        other: Label,
        is_greedy: bool,
    ) -> Label {
        let entry = self.fresh_label();
        self.emit_branch_epsilon_at(entry, prefer, other, is_greedy);
        entry
    }

    /// Emit an epsilon branch at a specific label.
    pub(super) fn emit_branch_epsilon_at(
        &mut self,
        label: Label,
        prefer: Label,
        other: Label,
        is_greedy: bool,
    ) {
        let successors = if is_greedy {
            vec![prefer, other]
        } else {
            vec![other, prefer]
        };
        self.emit_epsilon(label, successors);
    }
}
