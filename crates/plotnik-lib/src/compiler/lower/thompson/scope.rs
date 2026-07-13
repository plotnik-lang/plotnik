//! Scope management for structured captures.
//!
//! Handles `RecordOpen`/`RecordClose` and `ListOpen`/`ListClose` wrappers for captures.

use crate::bytecode::Nav;
use crate::compiler::ids::TypeId;
use crate::compiler::lower::ir::{EffectIR, Label, MatchIR, MemberRef};
use crate::compiler::parse::ast::Pattern;

use super::NfaBuilder;
use super::capture::{CaptureEffects, PatternCtx};

#[derive(Clone, Copy, Debug)]
pub struct RecordScope(pub TypeId);

/// Where a captured pattern's compiled scope continues.
///
/// Most captures have one continuation (`Single`). A capture wrapping an
/// optional/star at a navigating first-child position needs two: the parent must
/// restore the cursor and take a different path when the inner matches empty
/// times, so the match and skip paths exit separately (`Split`). A scope emitter
/// closes its scope on *every* continuation, so the only difference between the
/// two modes is how many close steps it emits. Threading this through one
/// mechanism dispatch (`compile_captured`) keeps the single- and split-exit paths
/// from drifting — the gap behind both #470 and the `@_` discard panic.
#[derive(Clone, Copy)]
pub(super) enum CaptureExits {
    /// One continuation for the matched path (a non-skippable capture, or the
    /// single-exit caller where match and skip coincide).
    Single(Label),
    /// Distinct continuations for the node-consuming path and the empty skip path.
    Split {
        match_exit: Label,
        skip_exit: SkipExit,
    },
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

    pub(super) fn map_targets(self, mut wrap: impl FnMut(Label) -> Label) -> Self {
        match self {
            Self::Single(exit) => Self::Single(wrap(exit)),
            Self::Split {
                match_exit,
                skip_exit,
            } => Self::Split {
                match_exit: wrap(match_exit),
                skip_exit: match skip_exit {
                    SkipExit::To(exit) => SkipExit::To(wrap(exit)),
                    SkipExit::Fail => SkipExit::Fail,
                },
            },
        }
    }
}

/// Continuation for an empty (skip) outcome.
///
/// `Fail` prunes the path: no continuation is emitted, so an empty outcome
/// backtracks like a plain match failure (the effect journal unwinds with it).
/// Quantifier iterations and alternation alternatives compile nullable elements
/// this way — there, consuming nothing is a failed attempt, never an empty
/// element or an empty win.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SkipExit {
    To(Label),
    Fail,
}

/// The two distinct continuations a skippable pattern (`?`/`*`) routes to:
/// the node-consuming path and the empty skip path. Bundling them keeps the two
/// from being transposed at a call site.
#[derive(Clone, Copy)]
pub(super) struct SplitExits {
    pub match_exit: Label,
    pub skip_exit: SkipExit,
}

/// The per-capture inputs shared by every capture lowering. The continuation is
/// not among them: it is a sibling argument whose type encodes the capability —
/// the scope-emitting helpers ([`NfaBuilder::compile_record_capture`] /
/// [`NfaBuilder::compile_list_capture`]) take a [`CaptureExits`], so an empty
/// can `Split` to a skip path; the non-scope pass-throughs (`Node`/`Ref`/
/// `PendingValue`) take a plain [`Label`] — they own no skip path, so a `Split` is
/// unrepresentable for them rather than silently collapsed via `match_exit`.
pub(super) struct CaptureRequest {
    pub inner: Pattern,
    pub nav: Option<Nav>,
    pub capture_effects: Vec<EffectIR>,
    pub outer_capture: CaptureEffects,
}

impl CaptureRequest {
    /// A definition-root list produces a pending value rather than assigning
    /// a named capture at this site.
    pub(super) fn pending_list(
        inner: Pattern,
        nav: Option<Nav>,
        outer_capture: CaptureEffects,
    ) -> Self {
        Self {
            inner,
            nav,
            capture_effects: vec![],
            outer_capture,
        }
    }
}

/// Emitted in order after a scope-closing epsilon (`ListClose`/`RecordClose`): the
/// capture's own value effects first, then the enclosing scope's. Bundled so the
/// two same-type slices can't be transposed at a call site.
#[derive(Clone, Copy)]
pub(super) struct ScopeCloseEffects<'a> {
    pub leading: &'a [EffectIR],
    pub capture: &'a [EffectIR],
    pub outer: &'a [EffectIR],
}

impl ScopeCloseEffects<'_> {
    pub(super) fn none() -> Self {
        Self {
            leading: &[],
            capture: &[],
            outer: &[],
        }
    }
}

impl NfaBuilder<'_> {
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
        self.scope_stack.push(RecordScope(type_id));
        let result = f(self);
        self.scope_stack.pop();
        result
    }

    /// Returns a `MemberRef` keyed by `(record_type, relative_index)`.
    pub(super) fn lookup_member(&self, capture_name: &str, type_id: TypeId) -> Option<MemberRef> {
        let fields = self.ctx.analysis.type_analysis.record_fields(type_id)?;
        for (relative_index, (&field_sym, _)) in fields.iter().enumerate() {
            if self.ctx.analysis.interner.resolve(field_sym) == capture_name {
                return Some(MemberRef::new(type_id, relative_index as u16));
            }
        }
        None
    }

    pub(super) fn lookup_member_in_scope(&self, capture_name: &str) -> Option<MemberRef> {
        let RecordScope(type_id) = *self.scope_stack.last()?;
        self.lookup_member(capture_name, type_id)
    }

    /// Compile a record-scope capture: `RecordOpen → inner → RecordClose+capture → exit(s)`.
    ///
    /// A quantified inner (`{...}? @cap`) routes to
    /// [`compile_optional_record_capture`](Self::compile_optional_record_capture): the
    /// record is optional as a whole, so the record scope must not open on the skip
    /// path. For the remaining (non-quantified) inners the record opens once and
    /// closes on every continuation.
    ///
    /// `outer_capture.pre` runs in the enclosing scope before the record opens
    /// (e.g. an alternative's default-value effects, or a variant's
    /// `VariantOpen`); dropping it would lose those and close a scope that never opened.
    /// `@cap` is resolved by the caller, against the enclosing scope.
    pub(super) fn compile_record_capture(
        &mut self,
        req: CaptureRequest,
        exits: CaptureExits,
    ) -> Label {
        // `{...}? @x`: the record is optional as a whole. The record scope moves
        // inside the quantifier iteration so a skip emits a bare `Absent` for the
        // capture — never a hollow `{ field: null }` record.
        if matches!(&req.inner, Pattern::QuantifiedPattern(_)) {
            return self.compile_optional_record_capture(req, exits);
        }

        let CaptureRequest {
            inner,
            nav,
            capture_effects,
            outer_capture,
        } = req;

        // The record scope's type drives the inner captures' `RecordSet` member resolution.
        let scope_type_id = self
            .ctx
            .analysis
            .type_analysis
            .expect_pattern_result(&inner)
            .flow
            .type_id();

        let end_effects = ScopeCloseEffects {
            leading: &[],
            capture: &capture_effects,
            outer: &outer_capture.post,
        };
        let inner_entry = match exits {
            CaptureExits::Single(exit) => {
                let record_close = self.emit_record_close_step_with_effects(end_effects, exit);
                self.compile_with_optional_scope(scope_type_id, |this| {
                    this.compile_pattern(&inner, record_close, nav)
                })
            }
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => {
                let match_record_close =
                    self.emit_record_close_step_with_effects(end_effects, match_exit);
                let skip_record_close = match skip_exit {
                    SkipExit::To(skip) => {
                        SkipExit::To(self.emit_record_close_step_with_effects(end_effects, skip))
                    }
                    SkipExit::Fail => SkipExit::Fail,
                };
                self.compile_with_optional_scope(scope_type_id, |this| {
                    let pattern_ctx = PatternCtx {
                        exit: match_record_close,
                        nav,
                        capture: CaptureEffects::default(),
                        value: false,
                    };
                    this.compile_nullable_pattern(&inner, pattern_ctx, skip_record_close)
                })
            }
        };

        self.emit_record_open_step_with_pre(inner_entry, outer_capture.pre)
    }

    /// Compile a node capture that also contains bubbling inner captures.
    ///
    /// `capture_effects` land on the inner match instruction, not a `RecordClose` step.
    /// The inner captures use the already-open outer scope, so no RecordOpen/RecordClose
    /// wrapper is emitted.
    pub(super) fn compile_bubble_with_node_capture(
        &mut self,
        req: CaptureRequest,
        exit: Label,
    ) -> Label {
        let CaptureRequest {
            inner,
            nav,
            capture_effects,
            outer_capture,
        } = req;

        let actual_exit = if outer_capture.post.is_empty() {
            exit
        } else {
            let outer_step = self.fresh_label();
            self.instructions.push(
                MatchIR::epsilon(outer_step, exit)
                    .append_effects(outer_capture.post)
                    .into(),
            );
            outer_step
        };

        let inner_capture = CaptureEffects::new(outer_capture.pre, capture_effects);
        let pattern_ctx = PatternCtx {
            exit: actual_exit,
            nav,
            capture: inner_capture,
            value: false,
        };
        self.dispatch_pattern(&inner, pattern_ctx)
    }

    /// Compile a pattern with RecordOpen/RecordClose wrapping for list iteration.
    ///
    /// Used when inner is a scope-creating pattern (sequence/alternation) with
    /// internal captures. Each iteration produces `RecordOpen → inner → RecordClose ArrayPush`.
    pub(super) fn compile_record_for_list(
        &mut self,
        inner: &Pattern,
        exit: Label,
        nav_override: Option<Nav>,
        element_type_id: Option<TypeId>,
    ) -> Label {
        let record_close_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(record_close_step, exit)
                .append_effect(EffectIR::record_close())
                .append_effect(EffectIR::array_push())
                .into(),
        );

        // `element_type_id` drives `RecordSet` effects inside the record scope.
        let inner_entry = self.compile_with_optional_scope(element_type_id, |this| {
            this.compile_iteration_element(
                inner,
                PatternCtx::with_nav(record_close_step, nav_override),
            )
        });

        let record_open_step = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(record_open_step, inner_entry)
                .prepend_effect(EffectIR::record_open())
                .into(),
        );

        record_open_step
    }

    pub(super) fn emit_list_close_step(
        &mut self,
        effects: ScopeCloseEffects<'_>,
        exit: Label,
    ) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, exit)
                .append_effects(effects.leading.iter().cloned())
                .append_effect(EffectIR::list_close())
                .append_effects(effects.capture.iter().cloned())
                .append_effects(effects.outer.iter().cloned())
                .into(),
        );
        label
    }

    /// Emit a record-open epsilon step with no enclosing pre-effects.
    pub(super) fn emit_record_open_step(&mut self, successor: Label) -> Label {
        self.emit_record_open_step_with_pre(successor, vec![])
    }

    /// Emit a record-close epsilon step with no capture or outer effects.
    pub(super) fn emit_record_close_step(&mut self, successor: Label) -> Label {
        self.emit_record_close_step_with_effects(ScopeCloseEffects::none(), successor)
    }

    /// Emit a record-close epsilon step carrying capture and outer effects.
    ///
    /// The record-scope counterpart of [`emit_list_close_step`](Self::emit_list_close_step),
    /// used by split-exit record captures to close the record and apply `RecordSet`
    /// on each exit.
    pub(super) fn emit_record_close_step_with_effects(
        &mut self,
        effects: ScopeCloseEffects<'_>,
        exit: Label,
    ) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, exit)
                .append_effects(effects.leading.iter().cloned())
                .append_effect(EffectIR::record_close())
                .append_effects(effects.capture.iter().cloned())
                .append_effects(effects.outer.iter().cloned())
                .into(),
        );
        label
    }

    /// Emit a list-open epsilon step with optional leading and trailing effects.
    pub(super) fn emit_list_open_step(
        &mut self,
        successor: Label,
        leading_effects: Vec<EffectIR>,
        trailing_effects: Vec<EffectIR>,
    ) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, successor)
                .prepend_effect(EffectIR::list_open())
                .prepend_effects(leading_effects)
                .append_effects(trailing_effects)
                .into(),
        );
        label
    }

    /// Emit a record-open epsilon step with optional leading effects.
    ///
    /// The record-scope counterpart of [`emit_list_open_step`](Self::emit_list_open_step),
    /// used by split-exit record captures to open the record after the enclosing
    /// scope's pre-effects (e.g. a variant case's `VariantOpen`).
    pub(super) fn emit_record_open_step_with_pre(
        &mut self,
        successor: Label,
        leading_effects: Vec<EffectIR>,
    ) -> Label {
        let label = self.fresh_label();
        self.instructions.push(
            MatchIR::epsilon(label, successor)
                .prepend_effect(EffectIR::record_open())
                .prepend_effects(leading_effects)
                .into(),
        );
        label
    }
}
