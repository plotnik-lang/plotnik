use crate::bytecode::{EffectKind, Nav};
use crate::compiler::lower::ir::{CallIR, CalleeEntry, EffectIR, Label, MatchIR, ReturnAddr};
use crate::compiler::parse::ast::{Pattern, QuantifierOperator};
use crate::core::NodeFieldId;

use super::NfaBuilder;
use super::capture::CaptureEffects;

/// `prefer` is tried first when greedy, `other` first when non-greedy. Bundled
/// so the two same-type `Label`s can't be transposed — a swap would silently
/// flip greediness.
#[derive(Clone, Copy)]
pub(super) struct BranchTargets {
    pub prefer: Label,
    pub other: Label,
}

#[derive(Clone, Copy)]
pub(super) enum Greediness {
    Greedy,
    NonGreedy,
}

impl From<QuantifierOperator> for Greediness {
    fn from(operator: QuantifierOperator) -> Self {
        if operator.is_greedy() {
            return Self::Greedy;
        }

        Self::NonGreedy
    }
}

impl Greediness {
    fn successors(self, targets: BranchTargets) -> Vec<Label> {
        let BranchTargets { prefer, other } = targets;
        match self {
            Self::Greedy => vec![prefer, other],
            Self::NonGreedy => vec![other, prefer],
        }
    }
}

impl NfaBuilder<'_> {
    pub(super) fn emit_call(
        &mut self,
        nav: Nav,
        node_field: Option<NodeFieldId>,
        return_addr: ReturnAddr,
        callee: CalleeEntry,
    ) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            CallIR::new(label, return_addr, callee)
                .nav(nav)
                .node_field(node_field)
                .into(),
        );
        label
    }

    /// Emit an epsilon with combined effects.
    ///
    /// Note: this consumes only `outer.post`. Callers whose capture owns no
    /// scope-opening step (`PendingValue`, suppressive) must route `outer.pre`
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
                .append_effects(effects)
                .append_effects(outer.post)
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
    /// own no such step — `PendingValue` and suppressive — have nowhere to fold it,
    /// so they call this. Dropping it loses an enum variant's `Enum`-open (or an
    /// union branch's null-injected defaults), and the path then closes a
    /// scope it never opened.
    pub(super) fn wrap_entry_pre(&mut self, entry: Label, pre: Vec<EffectIR>) -> Label {
        if pre.is_empty() {
            return entry;
        }
        let pre_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(pre_step, entry)
                .prepend_effects(pre)
                .into(),
        );
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

        self.emit_effects_if_nonempty(exit, null_effects)
    }

    /// Emit null effects for internal captures when skipping an optional/star pattern.
    ///
    /// Unlike `emit_null_for_skip_path` which handles captures passed as effects,
    /// this function handles captures defined INSIDE the pattern (e.g., `{(x) @cap}?`).
    /// It collects all capture names from the pattern and emits Null Set for each.
    pub(super) fn emit_null_for_internal_captures(
        &mut self,
        exit: Label,
        inner: &Pattern,
    ) -> Label {
        // Suppressed captures declare no fields; the name lookup below would
        // silently bind a same-named field of the enclosing scope.
        if self.is_suppressed() {
            return exit;
        }

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

        self.emit_effects_if_nonempty(exit, null_effects)
    }

    pub(super) fn emit_effects_if_nonempty(
        &mut self,
        exit: Label,
        effects: Vec<EffectIR>,
    ) -> Label {
        if effects.is_empty() {
            return exit;
        }

        self.emit_effects_epsilon(exit, effects, CaptureEffects::default())
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
    ///
    /// The nav is emitted exact: a wildcard step is always internal to an
    /// NFA-level retry loop (position search), and that loop owns the sibling
    /// search. The engine treats a non-exact `Down*`/`Next*` match acceptance
    /// as a choice point and leaves a resume checkpoint
    /// (`Nav::is_sibling_search`); an exact nav opts these steps out, keeping
    /// every search under exactly one retry owner. Behavior is otherwise
    /// identical — with an `Any` constraint the skip policy is never consulted.
    pub(super) fn emit_wildcard_nav(&mut self, label: Label, nav: Nav, successor: Label) {
        self.instructions.push(
            MatchIR::epsilon(label, successor)
                .nav(nav.to_exact())
                .into(),
        );
    }

    /// Emit an epsilon branch preferring `targets.prefer` when greedy,
    /// `targets.other` when non-greedy.
    pub(super) fn emit_branch_epsilon(
        &mut self,
        targets: BranchTargets,
        greediness: Greediness,
    ) -> Label {
        let entry = self.fresh_label();
        self.emit_branch_epsilon_at(entry, targets, greediness);
        entry
    }

    pub(super) fn emit_branch_epsilon_at(
        &mut self,
        label: Label,
        targets: BranchTargets,
        greediness: Greediness,
    ) {
        self.emit_epsilon(label, greediness.successors(targets));
    }

    /// Emit a resumable sibling search around a `body` that matches exactly at
    /// the current position. This is the single primitive for every kind of
    /// position search: navigate to a candidate, try the body there, and on
    /// failure advance to the next sibling and retry.
    ///
    /// ```text
    ///   navigate: Match(nav, wildcard) -> try
    ///   try:      epsilon -> [body, retry]
    ///   retry:    Match(Next, wildcard) -> try
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
            Greediness::Greedy,
        );

        let navigate = self.fresh_label();
        self.emit_wildcard_nav(navigate, nav, try_label);

        navigate
    }
}
