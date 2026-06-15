//! Scope management for structured captures.
//!
//! Handles Obj/EndObj and Arr/EndArr wrapper emission for struct and array captures.

use std::num::NonZeroU16;

use crate::analyze::type_check::TypeId;
use crate::bytecode::{CallIR, EffectIR, Label, MatchIR, MemberRef};
use crate::parser::Expr;
use plotnik_bytecode::{EffectOpcode, Nav};

use super::Compiler;
use super::capture::CaptureEffects;

/// Struct scope for tracking captures in nested contexts.
/// Each scope represents a struct type whose fields can receive captures.
#[derive(Clone, Copy, Debug)]
pub struct StructScope(pub TypeId);

/// Where a captured expression's compiled scope continues.
///
/// Most captures have one continuation (`Single`). A capture wrapping an
/// optional/star at a navigating first-child position needs two: the parent must
/// restore the cursor and take a different path when the inner matches zero
/// times, so the match and skip paths exit separately (`Split`). A scope emitter
/// closes its scope on *every* continuation, so the only difference between the
/// two modes is how many close steps it emits. Threading this through one
/// mechanism dispatch (`compile_captured`) keeps the single- and split-exit paths
/// from drifting — the gap behind both #470 and the suppressive `@_` panic.
#[derive(Clone, Copy)]
pub(super) enum CaptureExits {
    /// One continuation for the matched path (a non-skippable capture, or the
    /// single-exit caller where match and skip coincide).
    Single(Label),
    /// Distinct continuations for the matched path and the zero-match skip path.
    Split { match_exit: Label, skip_exit: Label },
}

impl CaptureExits {
    /// The continuation taken when the capture matches. For `Single` this is the
    /// only continuation; a bare capture (which never skips) uses it too.
    pub(super) fn match_exit(self) -> Label {
        match self {
            CaptureExits::Single(exit) => exit,
            CaptureExits::Split { match_exit, .. } => match_exit,
        }
    }
}

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

    /// Look up a capture name in a type, returning a member reference by
    /// (struct_type, relative_index).
    pub(super) fn lookup_member(&self, capture_name: &str, type_id: TypeId) -> Option<MemberRef> {
        let fields = self.ctx.type_ctx.get_struct_fields(type_id)?;
        for (relative_index, (&field_sym, _)) in fields.iter().enumerate() {
            if self.ctx.interner.resolve(field_sym) == capture_name {
                return Some(MemberRef::new(type_id, relative_index as u16));
            }
        }
        None
    }

    /// Look up a capture name in the current scope stack.
    pub(super) fn lookup_member_in_scope(&self, capture_name: &str) -> Option<MemberRef> {
        let StructScope(type_id) = *self.scope_stack.last()?;
        self.lookup_member(capture_name, type_id)
    }

    /// Compile a struct-scope capture: `Obj → inner → EndObj+capture → exit(s)`.
    ///
    /// The struct opens once and closes on every continuation. With `Single` exits
    /// the inner is compiled straight through; with `Split` exits (a `{...}? @cap`
    /// optional at a navigating first-child) the inner is compiled with split exits
    /// inside the scope and the struct closes on both — a skipped optional yields
    /// `{ field: null }`, never a bare null capture, so the child position stays
    /// consistent with the same pattern at the query root (#470).
    ///
    /// `outer_capture.pre` runs in the enclosing scope before the struct opens
    /// (e.g. an alternation branch's null-injected defaults, or a tagged variant's
    /// `Enum`); dropping it would lose those and close a scope that never opened.
    /// `@cap` is resolved by the caller, against the enclosing scope.
    pub(super) fn compile_struct_capture(
        &mut self,
        inner: &Expr,
        nav_override: Option<Nav>,
        scope_type_id: Option<TypeId>,
        capture_effects: Vec<EffectIR>,
        outer_capture: CaptureEffects,
        exits: CaptureExits,
    ) -> Label {
        let inner_entry = match exits {
            CaptureExits::Single(exit) => {
                let endobj =
                    self.emit_endobj_step_with_effects(&capture_effects, &outer_capture.post, exit);
                self.compile_with_optional_scope(scope_type_id, |this| {
                    this.compile_expr_with_nav(inner, endobj, nav_override)
                })
            }
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => {
                let match_endobj = self.emit_endobj_step_with_effects(
                    &capture_effects,
                    &outer_capture.post,
                    match_exit,
                );
                let skip_endobj = self.emit_endobj_step_with_effects(
                    &capture_effects,
                    &outer_capture.post,
                    skip_exit,
                );
                self.compile_with_optional_scope(scope_type_id, |this| {
                    this.compile_skippable_with_exits(
                        inner,
                        match_endobj,
                        skip_endobj,
                        nav_override,
                        CaptureEffects::default(),
                    )
                })
            }
        };

        self.emit_obj_step_with_pre(inner_entry, outer_capture.pre)
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
            let inner_capture = CaptureEffects::new(outer_capture.pre, capture_effects);
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
        let inner_capture = CaptureEffects::new_post(capture_effects);
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

    /// Emit an Obj epsilon step (no enclosing pre-effects).
    pub(super) fn emit_obj_step(&mut self, successor: Label) -> Label {
        self.emit_obj_step_with_pre(successor, vec![])
    }

    /// Emit an EndObj epsilon step (no capture or outer effects).
    pub(super) fn emit_endobj_step(&mut self, successor: Label) -> Label {
        self.emit_endobj_step_with_effects(&[], &[], successor)
    }

    /// Emit an EndObj epsilon step carrying capture + outer effects.
    ///
    /// The struct-scope counterpart of [`emit_endarr_step`](Self::emit_endarr_step),
    /// used by split-exit struct captures to close the object and Set the capture
    /// on each exit.
    pub(super) fn emit_endobj_step_with_effects(
        &mut self,
        capture_effects: &[EffectIR],
        outer_effects: &[EffectIR],
        exit: Label,
    ) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, exit)
                .post_effect(EffectIR::end_obj())
                .post_effects(capture_effects.iter().cloned())
                .post_effects(outer_effects.iter().cloned())
                .into(),
        );
        label
    }

    /// Emit an Arr epsilon step with optional pre-effects before start_arr.
    pub(super) fn emit_arr_step(&mut self, successor: Label, pre_effects: Vec<EffectIR>) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, successor)
                .pre_effects(pre_effects)
                .pre_effect(EffectIR::start_arr())
                .into(),
        );
        label
    }

    /// Emit an Obj epsilon step with optional pre-effects before start_obj.
    ///
    /// The struct-scope counterpart of [`emit_arr_step`](Self::emit_arr_step),
    /// used by split-exit struct captures to open the object after the enclosing
    /// scope's pre-effects (e.g. a tagged variant's `Enum`).
    pub(super) fn emit_obj_step_with_pre(
        &mut self,
        successor: Label,
        pre_effects: Vec<EffectIR>,
    ) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, successor)
                .pre_effects(pre_effects)
                .pre_effect(EffectIR::start_obj())
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
    ///
    /// Note: this consumes only `outer.post`. Callers whose capture owns no
    /// scope-opening step (`SetAfter`, suppressive) must route `outer.pre`
    /// separately via [`wrap_entry_pre`](Self::wrap_entry_pre).
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

    /// Emit `pre` effects on an epsilon that runs immediately before `entry`, in
    /// the enclosing scope. Returns the new entry, or `entry` unchanged when
    /// `pre` is empty.
    ///
    /// Scope-opening captures (`compile_struct_capture`, `compile_array_capture`)
    /// fold `outer_capture.pre` onto their own `Obj`/`Arr` step. Captures that
    /// own no such step — `SetAfter` and suppressive — have nowhere to fold it,
    /// so they call this. Dropping it loses a tagged variant's `Enum`-open (or an
    /// untagged branch's null-injected defaults), and the path then closes a
    /// scope it never opened.
    pub(super) fn wrap_entry_pre(&mut self, entry: Label, pre: Vec<EffectIR>) -> Label {
        if pre.is_empty() {
            return entry;
        }
        let pre_step = self.fresh_label();
        self.instructions
            .push(MatchIR::epsilon(pre_step, entry).pre_effects(pre).into());
        pre_step
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
            .filter(|eff| eff.opcode() == EffectOpcode::Set)
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
    /// Cascading for bytecode limits is handled by the lowering pass.
    pub(super) fn emit_epsilon(&mut self, label: Label, successors: Vec<Label>) {
        self.instructions
            .push(MatchIR::at(label).next_many(successors).into());
    }

    /// Emit a Match instruction.
    ///
    /// Cascading for bytecode limits is handled by the lowering pass.
    ///
    /// Returns the entry label (same as `instr.label`).
    pub(super) fn emit_match(&mut self, instr: MatchIR) -> Label {
        let entry = instr.label;
        self.instructions.push(instr.into());
        entry
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

    /// Emit a resumable sibling search around a `body` that matches exactly at
    /// the current position. This is the single primitive for every kind of
    /// position search: navigate to a candidate, try the body there, and on
    /// failure advance to the next sibling and retry.
    ///
    /// ```text
    ///   navigate: Match(nav, wildcard) → try
    ///   try:      epsilon → [body, retry]
    ///   retry:    Match(Next, wildcard) → try
    /// ```
    ///
    /// When the body fails (even deep inside, on a descendant), the VM
    /// backtracks to `try`, which falls through to `retry`, advances to the
    /// next sibling, and retries. When siblings are exhausted, backtracking
    /// propagates past `try` to the caller's checkpoint.
    ///
    /// The body is always preferred over advancing: the iteration has no exit
    /// edge of its own, so a following pattern can never bind at a
    /// failed-candidate cursor position (see #414). Greediness and zero-match
    /// escape, where applicable, live on the caller's loop-boundary epsilons,
    /// not here.
    ///
    /// Returns the `navigate` label (the entry point for the search).
    pub(super) fn emit_position_search(&mut self, nav: Nav, body: Label) -> Label {
        let try_label = self.fresh_label();

        let retry = self.fresh_label();
        self.emit_wildcard_nav(retry, Nav::Next, try_label);

        self.emit_branch_epsilon_at(try_label, body, retry, true);

        let navigate = self.fresh_label();
        self.emit_wildcard_nav(navigate, nav, try_label);

        navigate
    }
}
