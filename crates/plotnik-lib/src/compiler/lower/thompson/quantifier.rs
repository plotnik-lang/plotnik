//! Unified quantifier compilation (`?`, `*`, `+` and non-greedy variants).
//!
//! All quantifier paths — plain, array-context, and split-exit — share one
//! `compile_quantified_unified` entry point so greediness and search-nav logic
//! stay in one place.

use crate::bytecode::{EffectKind, Nav};
use crate::compiler::ids::TypeId;
use crate::compiler::lower::ir::{EffectIR, Label};
use crate::compiler::parse::ast::{self, Pattern, QuantifierKind, QuantifierOperator};

use super::NfaBuilder;
use super::capture::{CaptureEffects, PatternCtx, needs_struct_wrapper, row_type_id};
use super::navigation::resumable_search_nav;
use super::nfa_emit::{BranchTargets, Greediness};
use super::scope::{CaptureExits, CaptureRequest, ScopeCloseEffects, SplitExits};

/// The nav under which a quantifier iteration runs a resumable position search,
/// or `None` for a bounded anchor that matches a single candidate directly.
///
/// Identical to [`resumable_search_nav`] except `StayExact` is also a search.
/// The difference is the consumer, not the nav: a *match-once* item at
/// `StayExact` is positioned by an outer search and matches exactly there, but a
/// quantifier is a *loop* — even from a fixed `StayExact` start (a called def
/// body, an alternation candidate, the entrypoint) it must scan its siblings
/// forward, so it owns a resumable search. A bounded anchor (`*Skip*`/`*Exact`)
/// stays put in both. Folding this case back into `resumable_search_nav` makes
/// alternations double-wrap and regresses; the two must stay distinct.
pub(super) fn quantifier_search_nav(nav: Nav) -> Option<Nav> {
    match nav {
        Nav::StayExact => Some(Nav::StayExact),
        other => resumable_search_nav(Some(other)),
    }
}

/// Result of parsing a quantified pattern.
enum QuantifierForm {
    /// No inner pattern found.
    Empty,
    /// Inner pattern exists but no valid quantifier operator.
    Plain(Pattern),
    /// Valid quantified pattern with inner and kind.
    Quantified {
        inner: Pattern,
        kind: QuantifierOperator,
    },
}

fn classify_quantifier(quant: &ast::QuantifiedPattern) -> QuantifierForm {
    let Some(inner) = quant.inner() else {
        return QuantifierForm::Empty;
    };

    match quant.quantifier_operator() {
        Some(kind) => QuantifierForm::Quantified { inner, kind },
        None => QuantifierForm::Plain(inner),
    }
}

/// Whether a quantifier iterates inside an array capture. In `InArray` each
/// matched element gets a Push and rows scope to the array's row type; `Standalone`
/// is a plain `?`/`*`/`+` with no surrounding array.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ArrayContext {
    Standalone,
    InArray,
}

impl ArrayContext {
    fn is_in_array(self) -> bool {
        matches!(self, ArrayContext::InArray)
    }
}

#[derive(Clone)]
enum IterationScope {
    Standalone {
        capture: CaptureEffects,
    },
    StructWrapped {
        row_type_id: Option<TypeId>,
        capture: CaptureEffects,
    },
    RowScopedByIteration {
        row_type_id: Option<TypeId>,
        capture: CaptureEffects,
    },
    RowScopedByArrayExit {
        capture: CaptureEffects,
    },
}

impl IterationScope {
    fn for_iteration(
        inner: &Pattern,
        array_context: ArrayContext,
        capture: CaptureEffects,
        type_ctx: &crate::compiler::analyze::types::TypeAnalysis,
    ) -> Self {
        if !array_context.is_in_array() {
            return Self::Standalone { capture };
        }

        let row_type_id = row_type_id(inner, type_ctx);
        if needs_struct_wrapper(inner, type_ctx) {
            return Self::StructWrapped {
                row_type_id,
                capture,
            };
        }

        Self::RowScopedByIteration {
            row_type_id,
            capture,
        }
    }

    fn by_array_exit(&self) -> Self {
        match self {
            Self::Standalone { capture } => Self::Standalone {
                capture: capture.clone(),
            },
            Self::StructWrapped {
                row_type_id,
                capture,
            } => Self::StructWrapped {
                row_type_id: *row_type_id,
                capture: capture.clone(),
            },
            Self::RowScopedByIteration { capture, .. } => Self::RowScopedByArrayExit {
                capture: capture.clone(),
            },
            Self::RowScopedByArrayExit { capture } => Self::RowScopedByArrayExit {
                capture: capture.clone(),
            },
        }
    }

    fn capture(&self) -> &CaptureEffects {
        match self {
            Self::Standalone { capture }
            | Self::StructWrapped { capture, .. }
            | Self::RowScopedByIteration { capture, .. }
            | Self::RowScopedByArrayExit { capture } => capture,
        }
    }
}

#[derive(Clone, Copy)]
struct ExitNav {
    exit: Label,
    nav: Nav,
}

impl ExitNav {
    fn new(exit: Label, nav: Nav) -> Self {
        Self { exit, nav }
    }
}

/// Configuration for unified quantifier compilation.
pub(super) struct QuantifierConfig<'a> {
    pub inner: &'a Pattern,
    pub kind: QuantifierOperator,
    /// Navigation for the first iteration.
    pub first_nav: Option<Nav>,
    pub array_context: ArrayContext,
    pub element_capture: CaptureEffects,
    /// `Split` carries both the match and skip labels, so a skippable
    /// quantifier cannot be requested without a skip exit.
    pub exits: CaptureExits,
}

impl NfaBuilder<'_> {
    /// Compile a quantified pattern with capture effects (passed to body).
    pub(super) fn compile_quantified(
        &mut self,
        quant: &ast::QuantifiedPattern,
        ctx: PatternCtx,
    ) -> Label {
        let (inner, kind) = match classify_quantifier(quant) {
            QuantifierForm::Empty => return ctx.exit,
            QuantifierForm::Plain(inner) => return self.dispatch_pattern(&inner, ctx),
            QuantifierForm::Quantified { inner, kind } => (inner, kind),
        };

        let PatternCtx {
            exit,
            nav: nav_override,
            capture,
        } = ctx;

        let config = QuantifierConfig {
            inner: &inner,
            kind,
            first_nav: nav_override,
            array_context: ArrayContext::Standalone,
            element_capture: capture,
            exits: CaptureExits::Single(exit),
        };

        self.compile_quantified_unified(config)
    }

    /// Compile a quantified pattern for array capture with element-level effects.
    ///
    /// The element_capture effects (typically [Push]) are placed on each iteration.
    pub(super) fn compile_quantified_for_array(
        &mut self,
        quant: &ast::QuantifiedPattern,
        exit: Label,
        nav_override: Option<Nav>,
        element_capture: CaptureEffects,
    ) -> Label {
        let (inner, kind) = match classify_quantifier(quant) {
            QuantifierForm::Empty => return exit,
            QuantifierForm::Plain(inner) => {
                return self.dispatch_pattern(
                    &inner,
                    PatternCtx {
                        exit,
                        nav: nav_override,
                        capture: element_capture,
                    },
                );
            }
            QuantifierForm::Quantified { inner, kind } => (inner, kind),
        };

        let config = QuantifierConfig {
            inner: &inner,
            kind,
            first_nav: nav_override,
            array_context: ArrayContext::InArray,
            element_capture,
            exits: CaptureExits::Single(exit),
        };

        self.compile_quantified_unified(config)
    }

    /// Compile a skippable pattern (optional/star) with separate exits for skip/match paths.
    pub(super) fn compile_skippable_with_exits(
        &mut self,
        pattern: &Pattern,
        exits: SplitExits,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        let SplitExits {
            match_exit,
            skip_exit,
        } = exits;
        // A captured optional/star at this navigating position shares the single
        // mechanism dispatch with the ordinary capture path (`compile_captured`),
        // split exits and all, so the two can never drift — the gap behind both
        // #470 and the suppressive `@_` panic. It emits the scope that matches the
        // declared type (`Struct`/`Arr`/`Suppress`), closing it on both exits.
        if let Pattern::CapturedPattern(cap) = pattern
            && let Some(inner) = cap.inner()
        {
            return self.compile_captured(
                cap,
                Some(inner),
                nav_override,
                capture,
                CaptureExits::Split {
                    match_exit,
                    skip_exit,
                },
            );
        }

        // Must be a QuantifiedPattern at this point
        let Pattern::QuantifiedPattern(quant) = pattern else {
            return self.dispatch_pattern(
                pattern,
                PatternCtx {
                    exit: match_exit,
                    nav: nav_override,
                    capture,
                },
            );
        };

        let (inner, kind) = match classify_quantifier(quant) {
            QuantifierForm::Empty => return match_exit,
            QuantifierForm::Plain(inner) => {
                return self.dispatch_pattern(
                    &inner,
                    PatternCtx {
                        exit: match_exit,
                        nav: nav_override,
                        capture,
                    },
                );
            }
            QuantifierForm::Quantified { inner, kind } => (inner, kind),
        };

        let skip_with_null = self.emit_null_for_skip_path(skip_exit, &capture);
        let skip_with_internal_null = self.emit_null_for_internal_captures(skip_with_null, &inner);

        let config = QuantifierConfig {
            inner: &inner,
            kind,
            first_nav: nav_override,
            array_context: ArrayContext::Standalone,
            element_capture: capture,
            exits: CaptureExits::Split {
                match_exit,
                skip_exit: skip_with_internal_null,
            },
        };

        self.compile_quantified_unified(config)
    }

    /// Compile a struct-mechanism capture whose inner is an optional quantifier
    /// (`{...}? @x`, `[...]? @x`) — the only quantifier that reaches the struct
    /// mechanism, since `*`/`+` classify as `Array`.
    ///
    /// The row is optional as a whole. Mirroring how arrays scope each element
    /// row, the `Struct → body → EndStruct+Set` wrapper lives inside the
    /// iteration; the skip path emits a bare `Null` for the capture instead of
    /// a hollow `{ field: null }` struct, matching the declared
    /// `{ … } | null` type.
    pub(super) fn compile_optional_row_capture(
        &mut self,
        quant: &ast::QuantifiedPattern,
        nav_override: Option<Nav>,
        capture_effects: Vec<EffectIR>,
        outer_capture: CaptureEffects,
        exits: CaptureExits,
    ) -> Label {
        let QuantifierForm::Quantified { inner, kind } = classify_quantifier(quant) else {
            unreachable!("admitted struct-mechanism quantifier has an operator and an inner");
        };
        assert!(
            matches!(kind.kind(), QuantifierKind::Optional),
            "`*`/`+` captures classify as Array, never Struct"
        );

        let (match_exit, skip_exit) = match exits {
            CaptureExits::Single(exit) => (exit, exit),
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => (match_exit, skip_exit),
        };

        // Skip: the row is absent — null the capture; the enclosing scope's
        // trailing effects still run, as they do on the match path.
        let mut skip_effects: Vec<EffectIR> = capture_effects
            .iter()
            .filter(|eff| eff.kind() == EffectKind::Set)
            .flat_map(|set_eff| [EffectIR::null(), set_eff.clone()])
            .collect();
        skip_effects.extend(outer_capture.post.iter().cloned());
        let skip_target = self.emit_effects_if_nonempty(skip_exit, skip_effects);

        // The row scope's type drives the inner captures' Set member resolution.
        let row_type_id = self
            .ctx
            .analysis
            .type_analysis
            .expect_pattern_result(&inner)
            .flow
            .type_id();

        let end_effects = ScopeCloseEffects {
            capture: &capture_effects,
            outer: &outer_capture.post,
        };
        let iterate = self.emit_iteration(
            nav_override.unwrap_or(Nav::Down),
            match_exit,
            |this, target| {
                let ExitNav { exit, nav } = target;
                let struct_close = this.emit_struct_close_step_with_effects(end_effects, exit);
                let body = this.compile_with_optional_scope(row_type_id, |t| {
                    t.compile_pattern(&inner, struct_close, Some(nav))
                });
                this.emit_struct_step(body)
            },
        );

        let entry = self.emit_branch_epsilon(
            BranchTargets {
                prefer: iterate,
                other: skip_target,
            },
            Greediness::from(kind),
        );
        self.wrap_entry_pre(entry, outer_capture.pre)
    }

    /// Compile an array capture (`(x)* @cap`) — `Arr → quantifier (with Push)
    /// → EndArr+capture → exit(s)`. With `Single` exits the loop falls straight
    /// through; with `Split` exits a zero-match takes `skip_exit` and a loop-exit
    /// takes `match_exit`, each closing the array. `capture_effects` is built once
    /// by the caller; the matched element's `Node` is pushed only when the
    /// element is not already a structured value
    /// ([`quantifier_needs_node_for_push`](Self::quantifier_needs_node_for_push)).
    pub(super) fn compile_array_capture(
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
        let push_effects =
            CaptureEffects::new_post(if self.quantifier_needs_node_for_push(inner) {
                vec![EffectIR::node(), EffectIR::push()]
            } else {
                vec![EffectIR::push()]
            });

        let end_effects = ScopeCloseEffects {
            capture: &capture_effects,
            outer: &outer_capture.post,
        };
        let inner_entry = match exits {
            CaptureExits::Single(exit) => {
                let endarr = self.emit_endarr_step(end_effects, exit);
                if let Pattern::QuantifiedPattern(quant) = inner {
                    self.compile_quantified_for_array(quant, endarr, nav, push_effects)
                } else {
                    self.compile_pattern(inner, endarr, nav)
                }
            }
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => {
                let match_endarr = self.emit_endarr_step(end_effects, match_exit);
                let skip_endarr = self.emit_endarr_step(end_effects, skip_exit);
                self.compile_star_for_array_with_exits(
                    inner,
                    SplitExits {
                        match_exit: match_endarr,
                        skip_exit: skip_endarr,
                    },
                    nav,
                    push_effects,
                )
            }
        };

        // Emit Arr step at entry (with outer pre-effects like Enum)
        self.emit_arr_step(inner_entry, outer_capture.pre)
    }

    fn compile_star_for_array_with_exits(
        &mut self,
        pattern: &Pattern,
        exits: SplitExits,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        let SplitExits {
            match_exit,
            skip_exit,
        } = exits;
        let Pattern::QuantifiedPattern(quant) = pattern else {
            return self.dispatch_pattern(
                pattern,
                PatternCtx {
                    exit: match_exit,
                    nav: nav_override,
                    capture,
                },
            );
        };

        let (inner, kind) = match classify_quantifier(quant) {
            QuantifierForm::Empty => return match_exit,
            QuantifierForm::Plain(inner) => {
                return self.dispatch_pattern(
                    &inner,
                    PatternCtx {
                        exit: match_exit,
                        nav: nav_override,
                        capture,
                    },
                );
            }
            QuantifierForm::Quantified { inner, kind } => (inner, kind),
        };

        let config = QuantifierConfig {
            inner: &inner,
            kind,
            first_nav: nav_override,
            array_context: ArrayContext::InArray,
            element_capture: capture,
            exits: CaptureExits::Split {
                match_exit,
                skip_exit,
            },
        };

        self.compile_quantified_unified(config)
    }

    /// Emit a single quantifier iteration (`?`, or the leading match of `*`/`+`):
    /// reach one element via `nav`, match `compile_body` there, exit to `exit`.
    ///
    /// A search nav (see [`quantifier_search_nav`]) wraps a `StayExact` body in
    /// [`emit_position_search`](Self::emit_position_search), which owns a
    /// checkpoint so the search can resume at a later sibling when a following
    /// pattern fails. A bounded (anchored/exact) nav is applied to the body
    /// directly: it has a single candidate, so the VM's own `continue_search`
    /// enforces the skip policy and the iteration never advances past a named
    /// sibling.
    fn emit_iteration(
        &mut self,
        nav: Nav,
        exit: Label,
        compile_body: impl Fn(&mut Self, ExitNav) -> Label,
    ) -> Label {
        match quantifier_search_nav(nav) {
            Some(search) => {
                let body = compile_body(self, ExitNav::new(exit, Nav::StayExact));
                self.emit_position_search(search, body)
            }
            None => compile_body(self, ExitNav::new(exit, nav)),
        }
    }

    /// Emit the first and repeat iterations of a looping quantifier (`*`/`+`),
    /// both looping back to `loop_entry`.
    ///
    /// A resumable search nav shares one `StayExact` body behind two position
    /// searches, so the repeat reuses the body and resumes via the same
    /// mechanism as the first. A bounded nav instead compiles the body twice —
    /// the first iteration applies `first_nav`, the repeat applies its
    /// [`sibling_continuation`](Nav::sibling_continuation) — so repeated matches
    /// are bounded sibling steps (back-to-back) rather than a forward search.
    fn emit_loop_iterations(
        &mut self,
        first_nav: Nav,
        loop_entry: Label,
        compile_body: impl Fn(&mut Self, ExitNav) -> Label,
    ) -> (Label, Label) {
        let repeat_nav = first_nav.sibling_continuation();
        if let Some(first_search) = quantifier_search_nav(first_nav) {
            let body = compile_body(self, ExitNav::new(loop_entry, Nav::StayExact));
            // Invariant guarded by `quantifier_tests::search_nav_repeats_as_search`.
            let repeat_search =
                quantifier_search_nav(repeat_nav).expect("a search nav repeats as a search");
            (
                self.emit_position_search(first_search, body),
                self.emit_position_search(repeat_search, body),
            )
        } else {
            (
                compile_body(self, ExitNav::new(loop_entry, first_nav)),
                compile_body(self, ExitNav::new(loop_entry, repeat_nav)),
            )
        }
    }

    /// Single dispatch point for all quantifier shapes; see [`QuantifierConfig`] for knobs.
    fn compile_quantified_unified(&mut self, config: QuantifierConfig<'_>) -> Label {
        let QuantifierConfig {
            inner,
            kind,
            first_nav,
            array_context,
            element_capture,
            exits,
        } = config;

        let match_exit = exits.match_exit();

        let element_scope = IterationScope::for_iteration(
            inner,
            array_context,
            element_capture,
            self.ctx.analysis.type_analysis,
        );

        let compile_body = |this: &mut Self, target: ExitNav| -> Label {
            this.compile_quantified_body(inner, target, element_scope.clone())
        };

        let greediness = Greediness::from(kind);
        let first_nav_mode = first_nav.unwrap_or(Nav::Down);

        match kind.kind() {
            QuantifierKind::OneOrMore => {
                // Plus: must match at least once. The first iteration has no exit
                // fallback, so a total failure backtracks to the caller.
                let loop_entry = self.fresh_label();
                let (first_iterate, repeat_iterate) =
                    self.emit_loop_iterations(first_nav_mode, loop_entry, compile_body);

                self.emit_branch_epsilon_at(
                    loop_entry,
                    BranchTargets {
                        prefer: repeat_iterate,
                        other: match_exit,
                    },
                    greediness,
                );

                first_iterate
            }

            QuantifierKind::ZeroOrMore => match exits {
                CaptureExits::Split {
                    match_exit,
                    skip_exit,
                } => {
                    let split_scope = element_scope.by_array_exit();
                    let split_body = |this: &mut Self, target: ExitNav| -> Label {
                        this.compile_quantified_body(inner, target, split_scope.clone())
                    };

                    let loop_entry = self.fresh_label();
                    let (first_iterate, repeat_iterate) =
                        self.emit_loop_iterations(first_nav_mode, loop_entry, split_body);

                    self.emit_branch_epsilon_at(
                        loop_entry,
                        BranchTargets {
                            prefer: repeat_iterate,
                            other: match_exit,
                        },
                        greediness,
                    );

                    // zero-match backtracks to the entry epsilon's checkpoint, restoring
                    // the pre-nav cursor and taking skip_exit.
                    self.emit_branch_epsilon(
                        BranchTargets {
                            prefer: first_iterate,
                            other: skip_exit,
                        },
                        greediness,
                    )
                }
                CaptureExits::Single(_) => {
                    let loop_entry = self.fresh_label();
                    let (first_iterate, repeat_iterate) =
                        self.emit_loop_iterations(first_nav_mode, loop_entry, compile_body);

                    self.emit_branch_epsilon_at(
                        loop_entry,
                        BranchTargets {
                            prefer: repeat_iterate,
                            other: match_exit,
                        },
                        greediness,
                    );
                    self.emit_branch_epsilon(
                        BranchTargets {
                            prefer: first_iterate,
                            other: match_exit,
                        },
                        greediness,
                    )
                }
            },

            QuantifierKind::Optional => {
                let skip_with_null = match exits {
                    CaptureExits::Split { skip_exit, .. } => skip_exit,
                    CaptureExits::Single(_) => {
                        let null_exit =
                            self.emit_null_for_skip_path(match_exit, element_scope.capture());
                        self.emit_null_for_internal_captures(null_exit, inner)
                    }
                };

                // Any failure backtracks to the entry epsilon's checkpoint, restoring
                // the pre-navigation cursor and taking skip_with_null.
                let iterate = self.emit_iteration(first_nav_mode, match_exit, compile_body);
                self.emit_branch_epsilon(
                    BranchTargets {
                        prefer: iterate,
                        other: skip_with_null,
                    },
                    greediness,
                )
            }
        }
    }

    fn compile_quantified_body(
        &mut self,
        inner: &Pattern,
        target: ExitNav,
        element_scope: IterationScope,
    ) -> Label {
        let ExitNav { exit, nav } = target;
        match element_scope {
            IterationScope::Standalone { capture }
            | IterationScope::RowScopedByArrayExit { capture } => self.dispatch_pattern(
                inner,
                PatternCtx {
                    exit,
                    nav: Some(nav),
                    capture,
                },
            ),
            IterationScope::StructWrapped { row_type_id, .. } => {
                self.compile_struct_for_array(inner, exit, Some(nav), row_type_id)
            }
            IterationScope::RowScopedByIteration {
                row_type_id,
                capture,
            } => self.compile_with_optional_scope(row_type_id, |this| {
                this.dispatch_pattern(
                    inner,
                    PatternCtx {
                        exit,
                        nav: Some(nav),
                        capture,
                    },
                )
            }),
        }
    }
}
