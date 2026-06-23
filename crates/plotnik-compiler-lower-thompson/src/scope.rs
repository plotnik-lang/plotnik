//! Scope management for structured captures.
//!
//! Handles StructOpen/StructClose and Arr/EndArr wrapper emission for struct and array captures.

use std::num::NonZeroU16;

use plotnik_compiler_core::TypeId;
use plotnik_compiler_core::ir::{CallIR, EffectIR, Label, MatchIR, MemberRef};
use plotnik_compiler_core::Pattern;
use plotnik_bytecode::{EffectKind, Nav};

use super::Compiler;
use super::capture::{CaptureEffects, ExprCtx};

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

/// The two distinct continuations a skippable expression (`?`/`*`) routes to:
/// the matched path and the zero-match skip path. Bundling them keeps the two
/// adjacent `Label`s from being transposed at a call site.
#[derive(Clone, Copy)]
pub(super) struct SplitExits {
    pub match_exit: Label,
    pub skip_exit: Label,
}

/// The per-capture inputs shared by every capture lowering. The continuation is
/// not among them: it is a sibling argument whose type encodes the capability —
/// the scope-emitting helpers ([`Compiler::compile_struct_capture`] /
/// [`Compiler::compile_array_capture`]) take a [`CaptureExits`], so a zero-match
/// can `Split` to a skip path; the non-scope pass-throughs (`Node`/`Ref`/
/// `SetAfter`) take a plain [`Label`] — they own no skip path, so a `Split` is
/// unrepresentable for them rather than silently collapsed via `match_exit`.
pub(super) struct CaptureRequest<'a> {
    pub inner: &'a Pattern,
    pub nav: Option<Nav>,
    pub capture_effects: Vec<EffectIR>,
    pub outer_capture: CaptureEffects,
}

/// `prefer` is tried first when greedy, `other` first when non-greedy. Bundled
/// so the two same-type `Label`s can't be transposed — a swap would silently
/// flip greediness.
#[derive(Clone, Copy)]
pub(super) struct BranchTargets {
    pub prefer: Label,
    pub other: Label,
}

/// Emitted in order after a scope-closing epsilon (`EndArr`/`EndStruct`): the
/// capture's own value effects first, then the enclosing scope's. Bundled so the
/// two same-type slices can't be transposed at a call site.
#[derive(Clone, Copy)]
pub(super) struct EndScopeEffects<'a> {
    pub capture: &'a [EffectIR],
    pub outer: &'a [EffectIR],
}

impl EndScopeEffects<'_> {
    pub(super) fn none() -> Self {
        Self {
            capture: &[],
            outer: &[],
        }
    }
}

impl Compiler<'_> {
    /// Avoids the repeated `if let Some(type_id) = type_id { with_scope } else { f }` pattern.
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

    pub(super) fn with_scope<T>(&mut self, type_id: TypeId, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scope_stack.push(StructScope(type_id));
        let result = f(self);
        self.scope_stack.pop();
        result
    }

    /// Returns a `MemberRef` keyed by (struct_type, relative_index).
    pub(super) fn lookup_member(&self, capture_name: &str, type_id: TypeId) -> Option<MemberRef> {
        let fields = self.ctx.type_ctx.struct_fields(type_id)?;
        for (relative_index, (&field_sym, _)) in fields.iter().enumerate() {
            if self.ctx.interner.resolve(field_sym) == capture_name {
                return Some(MemberRef::new(type_id, relative_index as u16));
            }
        }
        None
    }

    pub(super) fn lookup_member_in_scope(&self, capture_name: &str) -> Option<MemberRef> {
        let StructScope(type_id) = *self.scope_stack.last()?;
        self.lookup_member(capture_name, type_id)
    }

    /// Compile a struct-scope capture: `Struct → inner → EndStruct+capture → exit(s)`.
    ///
    /// The struct opens once and closes on every continuation. With `Single` exits
    /// the inner is compiled straight through; with `Split` exits (a `{...}? @cap`
    /// optional at a navigating first-child) the inner is compiled with split exits
    /// inside the scope and the struct closes on both — a skipped optional yields
    /// `{ field: null }`, never a bare null capture, so the child position stays
    /// consistent with the same pattern at the query root (#470).
    ///
    /// `outer_capture.pre` runs in the enclosing scope before the struct opens
    /// (e.g. an alternation branch's null-injected defaults, or an enum variant's
    /// `Enum`); dropping it would lose those and close a scope that never opened.
    /// `@cap` is resolved by the caller, against the enclosing scope.
    pub(super) fn compile_struct_capture(
        &mut self,
        req: CaptureRequest<'_>,
        exits: CaptureExits,
    ) -> Label {
        let CaptureRequest {
            inner,
            nav,
            capture_effects,
            outer_capture,
        } = req;
        // The struct scope's type drives the inner captures' Set member resolution.
        let scope_type_id = self
            .ctx
            .type_ctx
            .pattern_result(inner)
            .expect("an analyzed scope inner has a pattern result")
            .flow
            .type_id();

        let end_effects = EndScopeEffects {
            capture: &capture_effects,
            outer: &outer_capture.post,
        };
        let inner_entry = match exits {
            CaptureExits::Single(exit) => {
                let struct_close = self.emit_struct_close_step_with_effects(end_effects, exit);
                self.compile_with_optional_scope(scope_type_id, |this| {
                    this.compile_pattern(inner, struct_close, nav)
                })
            }
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => {
                let match_struct_close = self.emit_struct_close_step_with_effects(end_effects, match_exit);
                let skip_struct_close = self.emit_struct_close_step_with_effects(end_effects, skip_exit);
                self.compile_with_optional_scope(scope_type_id, |this| {
                    this.compile_skippable_with_exits(
                        inner,
                        SplitExits {
                            match_exit: match_struct_close,
                            skip_exit: skip_struct_close,
                        },
                        nav,
                        CaptureEffects::default(),
                    )
                })
            }
        };

        self.emit_struct_step_with_pre(inner_entry, outer_capture.pre)
    }

    /// Compile a node capture that also contains bubbling inner captures.
    ///
    /// `capture_effects` land on the inner match instruction (not an EndStruct step).
    /// When `scope_type_id` is `None` the inner captures use the already-open outer
    /// scope (e.g., an array row struct) — no Struct/EndStruct is emitted. When it is
    /// `Some`, Struct/EndStruct wraps the inner to open a fresh struct scope.
    pub(super) fn compile_bubble_with_node_capture(
        &mut self,
        inner: &Pattern,
        exit: Label,
        nav_override: Option<Nav>,
        scope_type_id: Option<TypeId>,
        capture_effects: Vec<EffectIR>,
        outer_capture: CaptureEffects,
    ) -> Label {
        if scope_type_id.is_none() {
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

            let inner_capture = CaptureEffects::new(outer_capture.pre, capture_effects);
            return self.dispatch_pattern(
                inner,
                ExprCtx {
                    exit: actual_exit,
                    nav: nav_override,
                    capture: inner_capture,
                },
            );
        }

        // StructClose carries ONLY outer_capture effects (e.g. Push), not capture_effects,
        // which belong on the inner match instruction.
        let struct_close_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(struct_close_step, exit)
                .post_effect(EffectIR::end_struct())
                .post_effects(outer_capture.post)
                .into(),
        );

        // pre effects don't propagate through a StructOpen/StructClose scope wrapper.
        let inner_capture = CaptureEffects::new_post(capture_effects);
        let inner_entry = self.with_scope(
            scope_type_id.expect("scope_type_id is Some after the early return on None above"),
            |this| {
                this.dispatch_pattern(
                    inner,
                    ExprCtx {
                        exit: struct_close_step,
                        nav: nav_override,
                        capture: inner_capture,
                    },
                )
            },
        );

        let struct_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(struct_step, inner_entry)
                .pre_effect(EffectIR::start_struct())
                .into(),
        );

        struct_step
    }

    /// Compile an expression with Struct/EndStruct wrapping for array iteration.
    ///
    /// Used when inner is a scope-creating expression (sequence/alternation) with
    /// internal captures. Each iteration produces: Struct → inner → EndStruct Push
    pub(super) fn compile_struct_for_array(
        &mut self,
        inner: &Pattern,
        exit: Label,
        nav_override: Option<Nav>,
        row_type_id: Option<TypeId>,
    ) -> Label {
        let struct_close_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(struct_close_step, exit)
                .post_effect(EffectIR::end_struct())
                .post_effect(EffectIR::push())
                .into(),
        );

        // row_type_id drives Set effects inside the struct scope.
        let inner_entry = self.compile_with_optional_scope(row_type_id, |this| {
            this.compile_pattern(inner, struct_close_step, nav_override)
        });

        let struct_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(struct_step, inner_entry)
                .pre_effect(EffectIR::start_struct())
                .into(),
        );

        struct_step
    }

    pub(super) fn emit_endarr_step(&mut self, effects: EndScopeEffects<'_>, exit: Label) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, exit)
                .post_effect(EffectIR::end_arr())
                .post_effects(effects.capture.iter().cloned())
                .post_effects(effects.outer.iter().cloned())
                .into(),
        );
        label
    }

    /// Emit a struct epsilon step (no enclosing pre-effects).
    pub(super) fn emit_struct_step(&mut self, successor: Label) -> Label {
        self.emit_struct_step_with_pre(successor, vec![])
    }

    /// Emit a struct-close epsilon step (no capture or outer effects).
    pub(super) fn emit_struct_close_step(&mut self, successor: Label) -> Label {
        self.emit_struct_close_step_with_effects(EndScopeEffects::none(), successor)
    }

    /// Emit a struct-close epsilon step carrying capture + outer effects.
    ///
    /// The struct-scope counterpart of [`emit_endarr_step`](Self::emit_endarr_step),
    /// used by split-exit struct captures to close the struct and Set the capture
    /// on each exit.
    pub(super) fn emit_struct_close_step_with_effects(
        &mut self,
        effects: EndScopeEffects<'_>,
        exit: Label,
    ) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, exit)
                .post_effect(EffectIR::end_struct())
                .post_effects(effects.capture.iter().cloned())
                .post_effects(effects.outer.iter().cloned())
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

    /// Emit a struct epsilon step with optional pre-effects before start_struct.
    ///
    /// The struct-scope counterpart of [`emit_arr_step`](Self::emit_arr_step),
    /// used by split-exit struct captures to open the struct after the enclosing
    /// scope's pre-effects (e.g. an enum variant's `Enum`).
    pub(super) fn emit_struct_step_with_pre(
        &mut self,
        successor: Label,
        pre_effects: Vec<EffectIR>,
    ) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, successor)
                .pre_effects(pre_effects)
                .pre_effect(EffectIR::start_struct())
                .into(),
        );
        label
    }

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
    /// fold `outer_capture.pre` onto their own `Struct`/`Arr` step. Captures that
    /// own no such step — `SetAfter` and suppressive — have nowhere to fold it,
    /// so they call this. Dropping it loses an enum variant's `Enum`-open (or an
    /// union branch's null-injected defaults), and the path then closes a
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

    /// Null-inject captures on the skip path of an optional/star quantifier,
    /// mirroring what alternations do for asymmetric branches.
    /// Returns `exit` unchanged when no Set effects are present.
    pub(super) fn emit_null_for_skip_path(
        &mut self,
        exit: Label,
        capture: &CaptureEffects,
    ) -> Label {
        let null_effects: Vec<_> = capture
            .post
            .iter()
            .filter(|eff| eff.kind() == EffectKind::Set)
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
    pub(super) fn emit_null_for_internal_captures(&mut self, exit: Label, inner: &Pattern) -> Label {
        let captures = Self::collect_captures(inner);
        if captures.is_empty() {
            return exit;
        }

        let mut null_effects = Vec::new();
        for name in captures {
            if let Some(member_ref) = self.lookup_member_in_scope(&name) {
                null_effects.push(EffectIR::null());
                null_effects.push(EffectIR::with_member(EffectKind::Set, member_ref));
            }
        }

        if null_effects.is_empty() {
            return exit;
        }

        self.emit_effects_epsilon(exit, null_effects, CaptureEffects::default())
    }

    /// Cascading for bytecode limits is handled by the lowering pass.
    pub(super) fn emit_epsilon(&mut self, label: Label, successors: Vec<Label>) {
        self.instructions
            .push(MatchIR::terminal(label).successors(successors).into());
    }

    /// Cascading for bytecode limits is handled by the lowering pass.
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

    /// Emit an epsilon branch preferring `targets.prefer` when greedy,
    /// `targets.other` when non-greedy.
    pub(super) fn emit_branch_epsilon(&mut self, targets: BranchTargets, is_greedy: bool) -> Label {
        let entry = self.fresh_label();
        self.emit_branch_epsilon_at(entry, targets, is_greedy);
        entry
    }

    pub(super) fn emit_branch_epsilon_at(
        &mut self,
        label: Label,
        targets: BranchTargets,
        is_greedy: bool,
    ) {
        let BranchTargets { prefer, other } = targets;
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

        self.emit_branch_epsilon_at(
            try_label,
            BranchTargets {
                prefer: body,
                other: retry,
            },
            true,
        );

        let navigate = self.fresh_label();
        self.emit_wildcard_nav(navigate, nav, try_label);

        navigate
    }
}
