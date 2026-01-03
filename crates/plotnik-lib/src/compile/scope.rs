//! Scope management for structured captures.
//!
//! Handles Obj/EndObj and Arr/EndArr wrapper emission for struct and array captures.

use crate::bytecode::ir::{EffectIR, Instruction, Label, MatchIR, MemberRef};
use crate::bytecode::{EffectOpcode, Nav};
use crate::parser::ast::Expr;
use crate::analyze::type_check::TypeId;

use super::capture::CaptureEffects;
use super::Compiler;

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
    pub(super) fn lookup_member(&self, capture_name: &str, type_id: TypeId) -> Option<MemberRef> {
        let fields = self.type_ctx.get_struct_fields(type_id)?;
        for (relative_index, (&sym, _)) in fields.iter().enumerate() {
            if self.interner.resolve(sym) == capture_name {
                return Some(MemberRef::deferred(type_id, relative_index as u16));
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
        let mut end_effects = vec![EffectIR::simple(EffectOpcode::EndObj, 0)];
        end_effects.extend(capture_effects);
        end_effects.extend(outer_capture.post);

        let endobj_step = self.fresh_label();
        self.instructions.push(Instruction::Match(MatchIR {
            label: endobj_step,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: end_effects,
            successors: vec![exit],
        }));

        let inner_entry = self.compile_with_optional_scope(scope_type_id, |this| {
            this.compile_expr_with_nav(inner, endobj_step, nav_override)
        });

        let obj_step = self.fresh_label();
        self.instructions.push(Instruction::Match(MatchIR {
            label: obj_step,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![EffectIR::simple(EffectOpcode::Obj, 0)],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![inner_entry],
        }));

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
                self.instructions.push(Instruction::Match(MatchIR {
                    label: outer_step,
                    nav: Nav::Stay,
                    node_type: None,
                    node_field: None,
                    pre_effects: vec![],
                    neg_fields: vec![],
                    post_effects: outer_capture.post,
                    successors: vec![exit],
                }));
                outer_step
            };

            // Compile inner with capture_effects on the match instruction
            let inner_capture = CaptureEffects { post: capture_effects };
            return self.compile_expr_inner(inner, actual_exit, nav_override, inner_capture);
        }

        // When scope_type_id is Some, we need Obj/EndObj to create the scope
        // EndObj step with ONLY outer_capture effects (like Push), NOT capture_effects
        let mut end_effects = vec![EffectIR::simple(EffectOpcode::EndObj, 0)];
        end_effects.extend(outer_capture.post);

        let endobj_step = self.fresh_label();
        self.instructions.push(Instruction::Match(MatchIR {
            label: endobj_step,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: end_effects,
            successors: vec![exit],
        }));

        // Compile inner WITH capture_effects on the match instruction
        let inner_capture = CaptureEffects { post: capture_effects };
        let inner_entry = self.with_scope(scope_type_id.unwrap(), |this| {
            this.compile_expr_inner(inner, endobj_step, nav_override, inner_capture)
        });

        let obj_step = self.fresh_label();
        self.instructions.push(Instruction::Match(MatchIR {
            label: obj_step,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![EffectIR::simple(EffectOpcode::Obj, 0)],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![inner_entry],
        }));

        obj_step
    }

    /// Compile array scope: Arr → quantifier (with Push) → EndArr+capture → exit
    pub(super) fn compile_array_scope(
        &mut self,
        inner: &Expr,
        exit: Label,
        nav_override: Option<Nav>,
        capture_effects: Vec<EffectIR>,
        outer_capture: CaptureEffects,
    ) -> Label {
        let mut end_effects = vec![EffectIR::simple(EffectOpcode::EndArr, 0)];
        end_effects.extend(capture_effects);
        end_effects.extend(outer_capture.post);

        let endarr_step = self.fresh_label();
        self.instructions.push(Instruction::Match(MatchIR {
            label: endarr_step,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: end_effects,
            successors: vec![exit],
        }));

        let push_effects = CaptureEffects {
            post: if self.quantifier_needs_node_for_push(inner) {
                vec![
                    EffectIR::simple(EffectOpcode::Node, 0),
                    EffectIR::simple(EffectOpcode::Push, 0),
                ]
            } else {
                vec![EffectIR::simple(EffectOpcode::Push, 0)]
            },
        };
        let inner_entry = if let Expr::QuantifiedExpr(quant) = inner {
            self.compile_quantified_for_array(quant, endarr_step, nav_override, push_effects)
        } else {
            self.compile_expr_with_nav(inner, endarr_step, nav_override)
        };

        let arr_step = self.fresh_label();
        self.instructions.push(Instruction::Match(MatchIR {
            label: arr_step,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![EffectIR::simple(EffectOpcode::Arr, 0)],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![inner_entry],
        }));

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
        self.instructions.push(Instruction::Match(MatchIR {
            label: endobj_step,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![
                EffectIR::simple(EffectOpcode::EndObj, 0),
                EffectIR::simple(EffectOpcode::Push, 0),
            ],
            successors: vec![exit],
        }));

        // Compile inner with row scope (for Set effects to work)
        let inner_entry = self.compile_with_optional_scope(row_type_id, |this| {
            this.compile_expr_with_nav(inner, endobj_step, nav_override)
        });

        // Obj → inner_entry
        let obj_step = self.fresh_label();
        self.instructions.push(Instruction::Match(MatchIR {
            label: obj_step,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![EffectIR::simple(EffectOpcode::Obj, 0)],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![inner_entry],
        }));

        obj_step
    }

    /// Emit an EndArr epsilon step with the given effects.
    pub(super) fn emit_endarr_step(
        &mut self,
        capture_effects: &[EffectIR],
        outer_effects: &[EffectIR],
        exit: Label,
    ) -> Label {
        let mut effects = vec![EffectIR::simple(EffectOpcode::EndArr, 0)];
        effects.extend(capture_effects.iter().cloned());
        effects.extend(outer_effects.iter().cloned());

        let label = self.fresh_label();
        self.instructions.push(Instruction::Match(MatchIR {
            label,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: effects,
            successors: vec![exit],
        }));
        label
    }

    /// Emit an Arr epsilon step.
    pub(super) fn emit_arr_step(&mut self, successor: Label) -> Label {
        let label = self.fresh_label();
        self.instructions.push(Instruction::Match(MatchIR {
            label,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![EffectIR::simple(EffectOpcode::Arr, 0)],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![successor],
        }));
        label
    }

    /// Emit an epsilon with combined effects.
    pub(super) fn emit_effects_epsilon(
        &mut self,
        exit: Label,
        mut effects: Vec<EffectIR>,
        outer: CaptureEffects,
    ) -> Label {
        effects.extend(outer.post);
        let entry = self.fresh_label();
        self.instructions.push(Instruction::Match(MatchIR {
            label: entry,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: effects,
            successors: vec![exit],
        }));
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
    pub(super) fn emit_null_for_skip_path(&mut self, exit: Label, capture: &CaptureEffects) -> Label {
        // Collect Set effects - these are the fields that need nulling
        let null_effects: Vec<_> = capture
            .post
            .iter()
            .filter(|eff| eff.opcode == EffectOpcode::Set)
            .flat_map(|set_eff| {
                [
                    EffectIR::simple(EffectOpcode::Null, 0),
                    set_eff.clone(),
                ]
            })
            .collect();

        if null_effects.is_empty() {
            return exit;
        }

        let null_step = self.fresh_label();
        self.instructions.push(Instruction::Match(MatchIR {
            label: null_step,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: null_effects,
            successors: vec![exit],
        }));
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
                null_effects.push(EffectIR::simple(EffectOpcode::Null, 0));
                null_effects.push(EffectIR::with_member(EffectOpcode::Set, member_ref));
            }
        }

        if null_effects.is_empty() {
            return exit;
        }

        self.emit_effects_epsilon(exit, null_effects, CaptureEffects::default())
    }

    /// Emit an epsilon transition (no node interaction).
    pub(super) fn emit_epsilon(&mut self, label: Label, successors: Vec<Label>) {
        self.instructions.push(Instruction::Match(MatchIR {
            label,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors,
        }));
    }

    /// Emit an epsilon branch preferring `prefer` when greedy, `other` when non-greedy.
    pub(super) fn emit_branch_epsilon(&mut self, prefer: Label, other: Label, is_greedy: bool) -> Label {
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
