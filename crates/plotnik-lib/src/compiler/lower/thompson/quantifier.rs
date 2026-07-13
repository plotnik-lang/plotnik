//! Unified quantifier compilation (`?`, `*`, `+` and lazy variants).
//!
//! All quantifier paths — plain, list-context, and split-exit — share one
//! `compile_quantified` entry point so greediness and search-nav logic
//! stay in one place.

use std::collections::HashSet;

use crate::bytecode::{EffectKind, Nav, SpanKind};
use crate::compiler::analyze::types::CaptureTypePlan;
use crate::compiler::analyze::types::type_shape::PatternFlow;
use crate::compiler::ids::TypeId;
use crate::compiler::lower::ir::{EffectIR, InstructionIR, Label};
use crate::compiler::parse::ast::{self, Pattern, QuantifierKind, QuantifierOperator};

use super::NfaBuilder;
use super::capture::{
    CaptureEffects, PatternCtx, element_type_id, first_unmatched_close, needs_record_wrapper,
};
use super::navigation::resumable_search_nav;
use super::nfa_emit::{ForkTargets, Greediness};
use super::scope::{CaptureExits, CaptureRequest, ScopeCloseEffects, SkipExit, SplitExits};
use super::sequences::SeqItemsCtx;

/// The nav under which a quantifier iteration runs a resumable position search,
/// or `None` for a bounded anchor that matches a single candidate directly.
///
/// Identical to [`resumable_search_nav`] except `StayExact` is also a search.
/// The difference is the consumer, not the nav: a *match-once* item at
/// `StayExact` is positioned by an outer search and matches exactly there, but a
/// quantifier is a *loop* — even from a fixed `StayExact` start (a called def
/// body, an alternation candidate, the entry point) it must scan its siblings
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
pub(super) enum QuantifierForm {
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

pub(super) fn classify_quantifier(quant: &ast::QuantifiedPattern) -> QuantifierForm {
    let Some(inner) = quant.inner() else {
        return QuantifierForm::Empty;
    };

    match quant.quantifier_operator() {
        Some(kind) => QuantifierForm::Quantified { inner, kind },
        None => QuantifierForm::Plain(inner),
    }
}

#[derive(Clone)]
enum IterationScope {
    Standalone {
        capture: CaptureEffects,
    },
    RecordElement {
        element_type_id: Option<TypeId>,
        capture: CaptureEffects,
    },
    ElementScopeByIteration {
        element_type_id: Option<TypeId>,
        capture: CaptureEffects,
    },
    ElementScopeByListExit {
        capture: CaptureEffects,
    },
    CaptureType {
        plan: CaptureTypePlan,
        capture: CaptureEffects,
    },
}

impl IterationScope {
    fn standalone(capture: CaptureEffects) -> Self {
        Self::Standalone { capture }
    }

    fn list_element(
        inner: &Pattern,
        capture: CaptureEffects,
        type_ctx: &crate::compiler::analyze::types::TypeAnalysis,
    ) -> Self {
        let element_type_id = element_type_id(inner, type_ctx);
        if needs_record_wrapper(inner, type_ctx) {
            return Self::RecordElement {
                element_type_id,
                capture,
            };
        }

        Self::ElementScopeByIteration {
            element_type_id,
            capture,
        }
    }

    fn capture_type(plan: CaptureTypePlan) -> Self {
        Self::CaptureType {
            plan,
            capture: CaptureEffects::default(),
        }
    }

    fn by_list_exit(&self) -> Self {
        match self {
            Self::Standalone { capture } => Self::Standalone {
                capture: capture.clone(),
            },
            Self::RecordElement {
                element_type_id,
                capture,
            } => Self::RecordElement {
                element_type_id: *element_type_id,
                capture: capture.clone(),
            },
            Self::ElementScopeByIteration { capture, .. } => Self::ElementScopeByListExit {
                capture: capture.clone(),
            },
            Self::ElementScopeByListExit { capture } => Self::ElementScopeByListExit {
                capture: capture.clone(),
            },
            Self::CaptureType { plan, capture } => Self::CaptureType {
                plan: plan.clone(),
                capture: capture.clone(),
            },
        }
    }

    fn capture(&self) -> &CaptureEffects {
        match self {
            Self::Standalone { capture }
            | Self::RecordElement { capture, .. }
            | Self::ElementScopeByIteration { capture, .. }
            | Self::ElementScopeByListExit { capture }
            | Self::CaptureType { capture, .. } => capture,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ExitNav {
    pub(super) exit: Label,
    pub(super) nav: Nav,
}

struct QuantBrackets {
    inner_capture: CaptureEffects,
    exit: Label,
    entry_pre: Vec<EffectIR>,
    span_id: Option<u16>,
    closes: Vec<EffectIR>,
}

impl ExitNav {
    fn new(exit: Label, nav: Nav) -> Self {
        Self { exit, nav }
    }
}

#[derive(Clone, Copy)]
struct QuantifierRoute {
    first_nav: Option<Nav>,
    exits: CaptureExits,
}

impl QuantifierRoute {
    fn single(first_nav: Option<Nav>, exit: Label) -> Self {
        Self {
            first_nav,
            exits: CaptureExits::Single(exit),
        }
    }

    fn with_exits(first_nav: Option<Nav>, exits: CaptureExits) -> Self {
        Self { first_nav, exits }
    }
}

impl NfaBuilder<'_> {
    /// Split the incoming capture into loop-invariant brackets and per-iteration
    /// effects. `pre` (enclosing opens, null defaults) runs once before the
    /// first iteration; everything from the first unmatched close in `post`
    /// (closes of scopes this quantifier did not open) runs once after the
    /// last. Only the remaining `post` prefix — this level's own value effects —
    /// stays on each iteration. Without this split, a labeled alternative like
    /// `[A: (x)+ ...]` would re-open its variant scope on every loop pass and
    /// close it over the previous pass's pending value.
    fn bracket_quantifier(
        &mut self,
        quant: &ast::QuantifiedPattern,
        capture: CaptureEffects,
        exit: Label,
    ) -> QuantBrackets {
        let span_id = self
            .span_id(quant.syntax(), SpanKind::Quantifier)
            .map(|id| id.0);

        let mut post = capture.post;
        let closes = match first_unmatched_close(&post) {
            Some(split) => post.split_off(split),
            None => vec![],
        };
        let mut entry_pre = capture.pre;
        if let Some(id) = span_id {
            entry_pre.push(EffectIR::span_start(id));
        }
        let exit = self.quant_end(span_id, &closes, exit);

        QuantBrackets {
            inner_capture: CaptureEffects::new(vec![], post),
            exit,
            entry_pre,
            span_id,
            closes,
        }
    }

    fn quant_end_for(&mut self, brackets: &QuantBrackets, exit: Label) -> Label {
        self.quant_end(brackets.span_id, &brackets.closes, exit)
    }

    fn quant_end(&mut self, span_id: Option<u16>, closes: &[EffectIR], exit: Label) -> Label {
        let mut effects: Vec<EffectIR> = span_id.map(EffectIR::span_end).into_iter().collect();
        effects.extend(closes.iter().cloned());
        self.emit_effects_if_nonempty(exit, effects)
    }

    /// Whether this quantifier's value is observed by its continuation. The
    /// inferred `Value` flow is necessary but not enough: nested bare values are
    /// structural unless a root/capture/ref context consumes the pending value.
    fn is_value_collecting(&self, quant: &ast::QuantifiedPattern, value_context: bool) -> bool {
        if self.is_suppressed() || !value_context {
            return false;
        }
        let pattern = Pattern::QuantifiedPattern(quant.clone());
        matches!(
            self.ctx
                .analysis
                .type_analysis
                .expect_pattern_result(&pattern)
                .flow,
            PatternFlow::Value(_)
        )
    }

    /// Compile a quantified pattern with capture effects (passed to body).
    pub(super) fn compile_quantified_pattern(
        &mut self,
        quant: &ast::QuantifiedPattern,
        ctx: PatternCtx,
    ) -> Label {
        match classify_quantifier(quant) {
            QuantifierForm::Empty => return ctx.exit,
            QuantifierForm::Plain(inner) => return self.dispatch_pattern(&inner, ctx),
            QuantifierForm::Quantified { .. } => {}
        }

        let PatternCtx {
            exit,
            nav: nav_override,
            capture,
            value,
        } = ctx;
        let value_context = value || capture.post_consumes_value();

        if self.is_value_collecting(quant, value_context) {
            return self.compile_valued_quantifier(
                quant,
                CaptureExits::Single(exit),
                nav_override,
                capture,
            );
        }

        let brackets = self.bracket_quantifier(quant, capture, exit);

        let scope = IterationScope::standalone(brackets.inner_capture);
        let route = QuantifierRoute::single(nav_override, brackets.exit);
        let entry = self.compile_quantified(quant, scope, route);
        self.wrap_entry_pre(entry, brackets.entry_pre)
    }

    /// Compile a quantified pattern for list capture with element-level effects.
    ///
    /// The element-capture effects (typically `[ArrayPush]`) are placed on each iteration.
    pub(super) fn compile_quantified_for_list(
        &mut self,
        quant: &ast::QuantifiedPattern,
        exit: Label,
        nav_override: Option<Nav>,
        element_capture: CaptureEffects,
    ) -> Label {
        let inner = match classify_quantifier(quant) {
            QuantifierForm::Empty => return exit,
            QuantifierForm::Plain(inner) => {
                let pattern_ctx = PatternCtx {
                    exit,
                    nav: nav_override,
                    capture: element_capture,
                    value: false,
                };
                return self.dispatch_pattern(&inner, pattern_ctx);
            }
            QuantifierForm::Quantified { inner, .. } => inner,
        };
        let scope =
            IterationScope::list_element(&inner, element_capture, self.ctx.analysis.type_analysis);
        self.compile_quantified(quant, scope, QuantifierRoute::single(nav_override, exit))
    }

    pub(super) fn compile_capture_type_list_iterations(
        &mut self,
        quant: &ast::QuantifiedPattern,
        element: CaptureTypePlan,
        exits: CaptureExits,
        nav_override: Option<Nav>,
    ) -> Label {
        match classify_quantifier(quant) {
            QuantifierForm::Empty => return exits.match_exit(),
            QuantifierForm::Plain(inner) => {
                return self
                    .capture_type(
                        &element,
                        nav_override,
                        CaptureExits::Single(exits.match_exit()),
                    )
                    .list_item(&inner);
            }
            QuantifierForm::Quantified { .. } => {}
        }
        self.compile_quantified(
            quant,
            IterationScope::capture_type(element),
            QuantifierRoute::with_exits(nav_override, exits),
        )
    }

    /// Compile only the admitted empty outcomes of a nullable pattern.
    ///
    /// Ordinary skippable lowering already owns the exact value, scope, span,
    /// and recursive-reference semantics for every empty outcome. Compile
    /// that graph with both continuations joined, then project it onto control
    /// paths that never perform a node match. This retains both kinds of
    /// empty result — an absent option and a present structured value —
    /// without letting a lifted alternation fallback re-run its consuming body
    /// against the parent cursor.
    pub(super) fn compile_empty_outcome(&mut self, pattern: &Pattern, ctx: PatternCtx) -> Label {
        let exit = ctx.exit;
        let first_new_instruction = self.instructions.len();
        let entry = self.compile_nullable_pattern(pattern, ctx, SkipExit::To(exit));
        self.project_empty_paths(first_new_instruction, entry, exit)
    }

    /// Keep the newly emitted subgraph's paths that reach `exit` using only
    /// epsilon control flow or a childless anchor assertion. `Stay*` still
    /// counts as a node match: accepting it here is the bug that turns a
    /// wildcard under `?`'s absent value into its parent node.
    fn project_empty_paths(
        &mut self,
        first_new_instruction: usize,
        entry: Label,
        exit: Label,
    ) -> Label {
        let mut emitted = self.instructions.split_off(first_new_instruction);
        let mut reaches_exit = HashSet::from([exit]);

        loop {
            let before = reaches_exit.len();
            for instruction in &emitted {
                if !empty_path_control(instruction) {
                    continue;
                }
                if instruction
                    .successors()
                    .iter()
                    .any(|successor| reaches_exit.contains(successor))
                {
                    reaches_exit.insert(instruction.label());
                }
            }
            if reaches_exit.len() == before {
                break;
            }
        }

        assert!(
            reaches_exit.contains(&entry),
            "a nullable pattern must expose an admitted empty path"
        );

        for instruction in &mut emitted {
            let InstructionIR::Match(matched) = instruction else {
                continue;
            };
            matched
                .successors
                .retain(|successor| reaches_exit.contains(successor));
        }

        let mut reachable = HashSet::from([entry]);
        loop {
            let before = reachable.len();
            for instruction in &emitted {
                if !reachable.contains(&instruction.label())
                    || !reaches_exit.contains(&instruction.label())
                {
                    continue;
                }
                reachable.extend(instruction.successors().iter().copied());
            }
            if reachable.len() == before {
                break;
            }
        }

        emitted.retain(|instruction| {
            reaches_exit.contains(&instruction.label()) && reachable.contains(&instruction.label())
        });
        self.instructions.extend(emitted);
        entry
    }

    /// Compile a nullable pattern with independent node-consuming and empty
    /// continuations. The consuming continuation travels as a complete pattern
    /// context; the sibling argument is only the empty route.
    pub(super) fn compile_nullable_pattern(
        &mut self,
        pattern: &Pattern,
        matched: PatternCtx,
        skip_exit: SkipExit,
    ) -> Label {
        let PatternCtx {
            exit: match_exit,
            nav: nav_override,
            capture,
            value: value_context,
        } = self.bracket_pattern_ctx(pattern, matched);
        // A captured `?`/`*` at this navigating position shares the single
        // mechanism dispatch with the ordinary capture path (`compile_captured`),
        // split exits and all, so the two can never drift — the gap behind both
        // #470 and the `@_` discard panic. It emits the scope that matches the
        // declared scope (`Record`/`List`/`Suppress`), closing it on both exits.
        if let Pattern::CapturedPattern(cap) = pattern
            && cap.inner().is_some()
        {
            return self.compile_captured(
                cap,
                nav_override,
                capture,
                CaptureExits::Split {
                    match_exit,
                    skip_exit,
                },
            );
        }

        // A reference to a nullable definition skips like an inline `?`: its
        // body is inlined so the empty path takes `skip_exit` with the
        // checkpoint-restored cursor (a call's empty return cannot — the
        // return address carries the consumed-candidate navigation).
        if let Pattern::DefRef(r) = pattern {
            let def_id = self.resolve_ref_def_id(r);
            if self.nullable_defs.contains(&def_id) {
                let pattern_ctx = PatternCtx {
                    exit: match_exit,
                    nav: nav_override,
                    capture,
                    value: value_context,
                };
                return self.compile_ref_inline(def_id, pattern_ctx, skip_exit);
            }
        }

        // An alternation with a nullable alternative: the lifted empty
        // alternative exits to `skip_exit` (or is pruned) instead of
        // dead-ending inside the candidate search.
        if let Pattern::Alternation(alternation) = pattern {
            let pattern_ctx = PatternCtx {
                exit: match_exit,
                nav: nav_override,
                capture,
                value: value_context,
            };
            // Mirrors dispatch_pattern: only a consumed labeled alternation outside
            // suppression tags its variants.
            let flow = &self
                .ctx
                .analysis
                .type_analysis
                .expect_pattern_result(pattern)
                .flow;
            return if pattern_ctx.consumes_value()
                && matches!(flow, PatternFlow::Value(_))
                && !self.is_suppressed()
            {
                self.compile_labeled_alternation_with_exits(alternation, pattern_ctx, skip_exit)
            } else {
                self.compile_unlabeled_alternation_with_exits(alternation, pattern_ctx, skip_exit)
            };
        }

        // A group whose items can all skip: compile the items with the skip
        // exit threaded through, so the all-skip path exits with the
        // checkpoint-restored cursor exactly like a single skippable item
        // (partial matches exit through `match_exit` as usual).
        if let Pattern::SeqPattern(seq) = pattern {
            let items: Vec<_> = seq.items().collect();
            let is_inside_node = matches!(
                nav_override,
                Some(Nav::Down | Nav::DownSkip | Nav::DownSkipExtras | Nav::DownExact)
            );
            return self.compile_seq_items(SeqItemsCtx {
                items: &items,
                exit: match_exit,
                is_inside_node,
                first_nav: nav_override,
                capture,
                skip_exit: Some(skip_exit),
            });
        }

        // Must be a QuantifiedPattern at this point: every caller passes a
        // nullable pattern, and every other nullable form early-returned above.
        // Falling through to `dispatch_pattern` here would bracket the pattern's
        // inspection span a second time (it was already bracketed on entry).
        let Pattern::QuantifiedPattern(quant) = pattern else {
            unreachable!("skippable dispatch fell through for non-nullable pattern kind");
        };

        let inner = match classify_quantifier(quant) {
            QuantifierForm::Empty => return match_exit,
            QuantifierForm::Plain(inner) => {
                let pattern_ctx = PatternCtx {
                    exit: match_exit,
                    nav: nav_override,
                    capture,
                    value: value_context,
                };
                return self.dispatch_pattern(&inner, pattern_ctx);
            }
            QuantifierForm::Quantified { inner, .. } => inner,
        };

        if self.is_value_collecting(quant, value_context || capture.post_consumes_value()) {
            return self.compile_valued_quantifier(
                quant,
                CaptureExits::Split {
                    match_exit,
                    skip_exit,
                },
                nav_override,
                capture,
            );
        }

        let brackets = self.bracket_quantifier(quant, capture, match_exit);

        let skip_exit = match skip_exit {
            SkipExit::To(skip) => {
                let quant_end = self.quant_end_for(&brackets, skip);
                let skip_with_absence =
                    self.emit_absence_for_skip_path(quant_end, &brackets.inner_capture);
                SkipExit::To(self.emit_absence_for_internal_captures(skip_with_absence, &inner))
            }
            SkipExit::Fail => SkipExit::Fail,
        };

        let scope = IterationScope::standalone(brackets.inner_capture);
        let route = QuantifierRoute::with_exits(
            nav_override,
            CaptureExits::Split {
                match_exit: brackets.exit,
                skip_exit,
            },
        );
        let entry = self.compile_quantified(quant, scope, route);
        self.wrap_entry_pre(entry, brackets.entry_pre)
    }

    /// Compile a record capture whose inner has a `?` quantifier
    /// (`{...}? @x`, `[...]? @x`) — the only quantifier that reaches the record
    /// mechanism, since `*`/`+` classify as `List`.
    ///
    /// The record value has option type. Mirroring how lists scope each element
    /// record, the `RecordOpen → body → RecordClose+RecordSet` wrapper lives inside the
    /// iteration; the skip path emits a bare `Absent` for the capture instead of
    /// a hollow `{ field: null }` record, matching the declared
    /// `{ … } | null` type.
    pub(super) fn compile_option_record_capture(
        &mut self,
        req: CaptureRequest,
        exits: CaptureExits,
    ) -> Label {
        let CaptureRequest {
            inner: Pattern::QuantifiedPattern(quant),
            nav: nav_override,
            capture_effects,
            outer_capture,
        } = req
        else {
            unreachable!("option-record lowering receives a quantified capture")
        };
        let QuantifierForm::Quantified { inner, kind } = classify_quantifier(&quant) else {
            unreachable!("admitted record capture has an operator and an inner");
        };
        assert!(
            matches!(kind.kind(), QuantifierKind::Optional),
            "`*`/`+` captures classify as List, never Record"
        );

        let (match_exit, skip_exit) = match exits {
            CaptureExits::Single(exit) => (exit, SkipExit::To(exit)),
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => (match_exit, skip_exit),
        };
        let brackets = self.bracket_quantifier(&quant, outer_capture, match_exit);

        // Skip: the record is absent — null the capture; the enclosing scope's
        // trailing effects still run, as they do on the match path.
        let skip_target = match skip_exit {
            SkipExit::To(skip) => {
                let quant_end = self.quant_end_for(&brackets, skip);
                let mut skip_effects: Vec<EffectIR> = capture_effects
                    .iter()
                    .filter(|eff| eff.kind() == EffectKind::RecordSet)
                    .flat_map(|set_eff| [EffectIR::absent(), set_eff.clone()])
                    .collect();
                skip_effects.extend(brackets.inner_capture.post.iter().cloned());
                Some(self.emit_effects_if_nonempty(quant_end, skip_effects))
            }
            SkipExit::Fail => None,
        };

        // The record's type drives the inner captures' `RecordSet` member resolution.
        let record_type_id = self
            .ctx
            .analysis
            .type_analysis
            .expect_pattern_result(&inner)
            .flow
            .type_id();

        let end_effects = ScopeCloseEffects {
            leading: &[],
            capture: &capture_effects,
            outer: &brackets.inner_capture.post,
        };
        let iterate = self.emit_iteration(
            nav_override.unwrap_or(Nav::Down),
            brackets.exit,
            |this, target| {
                let ExitNav { exit, nav } = target;
                let record_close = this.emit_record_close_with_effects(end_effects, exit);
                let body = this.with_scope_if_present(record_type_id, |t| {
                    t.compile_iteration_element(
                        &inner,
                        PatternCtx::with_nav(record_close, Some(nav)),
                    )
                });
                this.emit_record_open(body)
            },
        );

        let entry = match skip_target {
            Some(skip_target) => self.emit_fork_epsilon(
                ForkTargets {
                    prefer: iterate,
                    other: skip_target,
                },
                Greediness::from(kind),
            ),
            // Pruned: the element must match — an empty outcome backtracks.
            None => iterate,
        };
        self.wrap_entry_pre(entry, brackets.entry_pre)
    }

    /// Compile a list capture (`(x)* @cap`) — `ListOpen → quantifier (with ArrayPush)
    /// → ListClose+capture → exit(s)`. With `Single` exits the loop falls straight
    /// through; with `Split` exits an empty match takes `skip_exit` and a loop-exit
    /// takes `match_exit`, each closing the list. `capture_effects` is built once
    /// by the caller; the matched element's `Node` is pushed only when the
    /// element is not already a structured value
    /// ([`quantifier_needs_node_for_push`](Self::quantifier_needs_node_for_push)).
    pub(super) fn compile_list_capture(
        &mut self,
        req: CaptureRequest,
        exits: CaptureExits,
    ) -> Label {
        let CaptureRequest {
            inner,
            nav,
            capture_effects,
            outer_capture,
        } = req;
        let push_effects =
            CaptureEffects::new_post(if self.quantifier_needs_node_for_push(&inner) {
                vec![EffectIR::node(), EffectIR::array_push()]
            } else {
                vec![EffectIR::array_push()]
            });

        let mut quant_start = Vec::new();
        let mut quant_end = Vec::new();
        if let Pattern::QuantifiedPattern(quant) = &inner
            && let Some(id) = self.span_id(quant.syntax(), SpanKind::Quantifier)
        {
            quant_start.push(EffectIR::span_start(id.0));
            quant_end.push(EffectIR::span_end(id.0));
        }

        let end_effects = ScopeCloseEffects {
            leading: &quant_end,
            capture: &capture_effects,
            outer: &outer_capture.post,
        };
        let inner_entry = match exits {
            CaptureExits::Single(exit) => {
                let list_close = self.emit_list_close(end_effects, exit);
                if let Pattern::QuantifiedPattern(quant) = &inner {
                    self.compile_quantified_for_list(quant, list_close, nav, push_effects)
                } else {
                    self.compile_pattern(&inner, list_close, nav)
                }
            }
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => {
                let match_list_close = self.emit_list_close(end_effects, match_exit);
                let skip_list_close = match skip_exit {
                    SkipExit::To(skip) => SkipExit::To(self.emit_list_close(end_effects, skip)),
                    SkipExit::Fail => SkipExit::Fail,
                };
                self.compile_star_for_list_with_exits(
                    &inner,
                    SplitExits {
                        match_exit: match_list_close,
                        skip_exit: skip_list_close,
                    },
                    nav,
                    push_effects,
                )
            }
        };

        // Emit the list-open state at entry with outer pre-effects such as VariantOpen.
        self.emit_list_open(inner_entry, outer_capture.pre, quant_start)
    }

    fn compile_star_for_list_with_exits(
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
            let pattern_ctx = PatternCtx {
                exit: match_exit,
                nav: nav_override,
                capture,
                value: false,
            };
            return self.dispatch_pattern(pattern, pattern_ctx);
        };

        let inner = match classify_quantifier(quant) {
            QuantifierForm::Empty => return match_exit,
            QuantifierForm::Plain(inner) => {
                let pattern_ctx = PatternCtx {
                    exit: match_exit,
                    nav: nav_override,
                    capture,
                    value: false,
                };
                return self.dispatch_pattern(&inner, pattern_ctx);
            }
            QuantifierForm::Quantified { inner, .. } => inner,
        };

        let scope = IterationScope::list_element(&inner, capture, self.ctx.analysis.type_analysis);
        let route = QuantifierRoute::with_exits(
            nav_override,
            CaptureExits::Split {
                match_exit,
                skip_exit,
            },
        );
        self.compile_quantified(quant, scope, route)
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
    pub(super) fn emit_iteration(
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
    /// are adjacent sibling matches rather than a forward search.
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

    /// Single dispatch point for every quantified shape. Callers choose the
    /// semantic iteration scope and route; the quantifier itself remains the
    /// authority for its element and greediness.
    fn compile_quantified(
        &mut self,
        quant: &ast::QuantifiedPattern,
        element_scope: IterationScope,
        route: QuantifierRoute,
    ) -> Label {
        let QuantifierForm::Quantified { inner, kind } = classify_quantifier(quant) else {
            unreachable!("core quantifier lowering receives a validated operator")
        };
        let QuantifierRoute { first_nav, exits } = route;

        let match_exit = exits.match_exit();

        let compile_body = |this: &mut Self, target: ExitNav| -> Label {
            this.compile_quantified_body(&inner, target, element_scope.clone())
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

                self.emit_fork_epsilon_at(
                    loop_entry,
                    ForkTargets {
                        prefer: repeat_iterate,
                        other: match_exit,
                    },
                    greediness,
                );

                first_iterate
            }

            QuantifierKind::ZeroOrMore => match exits {
                // Pruned empty match: the star must consume, so it compiles
                // exactly like a plus — total failure backtracks to the caller.
                CaptureExits::Split {
                    match_exit,
                    skip_exit: SkipExit::Fail,
                } => {
                    let loop_entry = self.fresh_label();
                    let (first_iterate, repeat_iterate) =
                        self.emit_loop_iterations(first_nav_mode, loop_entry, compile_body);

                    self.emit_fork_epsilon_at(
                        loop_entry,
                        ForkTargets {
                            prefer: repeat_iterate,
                            other: match_exit,
                        },
                        greediness,
                    );

                    first_iterate
                }
                CaptureExits::Split {
                    match_exit,
                    skip_exit: SkipExit::To(skip_exit),
                } => {
                    let split_scope = element_scope.by_list_exit();
                    let split_body = |this: &mut Self, target: ExitNav| -> Label {
                        this.compile_quantified_body(&inner, target, split_scope.clone())
                    };

                    let loop_entry = self.fresh_label();
                    let (first_iterate, repeat_iterate) =
                        self.emit_loop_iterations(first_nav_mode, loop_entry, split_body);

                    self.emit_fork_epsilon_at(
                        loop_entry,
                        ForkTargets {
                            prefer: repeat_iterate,
                            other: match_exit,
                        },
                        greediness,
                    );

                    // An empty match backtracks to the entry epsilon's checkpoint, restoring
                    // the pre-nav cursor and taking skip_exit.
                    self.emit_fork_epsilon(
                        ForkTargets {
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

                    self.emit_fork_epsilon_at(
                        loop_entry,
                        ForkTargets {
                            prefer: repeat_iterate,
                            other: match_exit,
                        },
                        greediness,
                    );
                    self.emit_fork_epsilon(
                        ForkTargets {
                            prefer: first_iterate,
                            other: match_exit,
                        },
                        greediness,
                    )
                }
            },

            QuantifierKind::Optional => {
                let skip_with_absence = match exits {
                    CaptureExits::Split {
                        skip_exit: SkipExit::To(skip_exit),
                        ..
                    } => skip_exit,
                    // Pruned: the element must match — no skip alternative.
                    CaptureExits::Split {
                        skip_exit: SkipExit::Fail,
                        ..
                    } => {
                        return self.emit_iteration(first_nav_mode, match_exit, compile_body);
                    }
                    CaptureExits::Single(_) => {
                        let absence_exit =
                            self.emit_absence_for_skip_path(match_exit, element_scope.capture());
                        self.emit_absence_for_internal_captures(absence_exit, &inner)
                    }
                };

                // Any failure backtracks to the entry epsilon's checkpoint, restoring
                // the pre-navigation cursor and taking `skip_with_absence`.
                let iterate = self.emit_iteration(first_nav_mode, match_exit, compile_body);
                self.emit_fork_epsilon(
                    ForkTargets {
                        prefer: iterate,
                        other: skip_with_absence,
                    },
                    greediness,
                )
            }
        }
    }

    /// Compile a quantifier that IS a definition's output: the collected
    /// value is left pending as the call's return value — a captured
    /// quantifier with no consumer of its own. `*`/`+` collect a list
    /// (`ListOpen → iterations with ArrayPush → ListClose`); `?` leaves the element's
    /// value pending on the match path and a bare `Absent` on the skip path.
    fn compile_valued_quantifier(
        &mut self,
        quant: &ast::QuantifiedPattern,
        exits: CaptureExits,
        nav_override: Option<Nav>,
        outer: CaptureEffects,
    ) -> Label {
        let QuantifierForm::Quantified { inner: _, kind } = classify_quantifier(quant) else {
            unreachable!("a value-collecting quantifier has an operator and an inner");
        };

        match kind.kind() {
            QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore => {
                let pattern = Pattern::QuantifiedPattern(quant.clone());
                let req = CaptureRequest::pending_list(pattern, nav_override, outer);
                self.compile_list_capture(req, exits)
            }
            QuantifierKind::Optional => {
                self.compile_valued_optional(quant, exits, nav_override, outer)
            }
        }
    }

    /// The `?` half of [`compile_valued_quantifier`](Self::compile_valued_quantifier).
    ///
    /// The element's value must survive as the pending call value, so a
    /// reference element compiles with the keep-value ref lowering (a plain
    /// `RecordSet`-consumer chain doesn't exist at a definition's root); a node
    /// element pends its match via a `Node` effect, while structured
    /// elements leave their own value pending.
    fn compile_valued_optional(
        &mut self,
        quant: &ast::QuantifiedPattern,
        exits: CaptureExits,
        nav_override: Option<Nav>,
        outer: CaptureEffects,
    ) -> Label {
        let QuantifierForm::Quantified { inner, kind } = classify_quantifier(quant) else {
            unreachable!("a value-producing `?` quantifier has an operator and an inner pattern")
        };
        assert_eq!(kind.kind(), QuantifierKind::Optional);
        let (match_exit, skip_exit) = match exits {
            CaptureExits::Single(exit) => (exit, SkipExit::To(exit)),
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => (match_exit, skip_exit),
        };
        let brackets = self.bracket_quantifier(quant, outer, match_exit);

        // Skip: the value is a bare null; enclosing-scope effects still run.
        let skip_target = match skip_exit {
            SkipExit::To(skip) => {
                let quant_end = self.quant_end_for(&brackets, skip);
                let mut skip_effects = vec![EffectIR::absent()];
                skip_effects.extend(brackets.inner_capture.post.iter().cloned());
                Some(self.emit_effects_if_nonempty(quant_end, skip_effects))
            }
            SkipExit::Fail => None,
        };
        let match_target =
            self.emit_effects_if_nonempty(brackets.exit, brackets.inner_capture.post.clone());

        // A field constraint is navigation on the element, not structure.
        let (element, field_override) = match &inner {
            Pattern::FieldPattern(f) => {
                let field_id = self.resolve_field(f);
                match f.value() {
                    Some(v) => (v, field_id),
                    None => (inner.clone(), None),
                }
            }
            other => (other.clone(), None),
        };

        let first_nav = nav_override.unwrap_or(Nav::Down);
        let iterate = if let Pattern::DefRef(r) = &element {
            let def_id = self.resolve_ref_def_id(r);
            if self.nullable_defs.contains(&def_id) {
                // An empty element match and a skip of the `?` both leave
                // a null pending; funneling the inline skip into the match
                // continuation keeps the two paths one value.
                self.emit_iteration(first_nav, match_target, |this, target| {
                    let ExitNav { exit, nav } = target;
                    this.compile_ref_inline_keep_value(
                        def_id,
                        SplitExits {
                            match_exit: exit,
                            skip_exit: SkipExit::To(exit),
                        },
                        Some(nav),
                    )
                })
            } else {
                self.emit_iteration(first_nav, match_target, |this, target| {
                    let ExitNav { exit, nav } = target;
                    this.compile_ref_call_keep_value(def_id, exit, Some(nav), field_override)
                })
            }
        } else {
            let needs_node = self.element_needs_node(&inner);
            self.emit_iteration(first_nav, match_target, |this, target| {
                let ExitNav { exit, nav } = target;
                let post = if needs_node {
                    vec![EffectIR::node()]
                } else {
                    vec![]
                };
                this.compile_iteration_element(
                    &element,
                    PatternCtx {
                        exit,
                        nav: Some(nav),
                        capture: CaptureEffects::new_post(post),
                        value: !needs_node,
                    },
                )
            })
        };

        let entry = match skip_target {
            Some(skip_target) => self.emit_fork_epsilon(
                ForkTargets {
                    prefer: iterate,
                    other: skip_target,
                },
                Greediness::from(kind),
            ),
            // Pruned: the value must match — an empty outcome backtracks.
            None => iterate,
        };
        self.wrap_entry_pre(entry, brackets.entry_pre)
    }

    /// Compile one quantifier-iteration element. A nullable element compiles
    /// with its empty path pruned ([`SkipExit::Fail`]): an iteration that
    /// consumes nothing is a failed attempt — the search advances or the loop
    /// exits — never a spurious empty element.
    pub(super) fn compile_iteration_element(&mut self, inner: &Pattern, ctx: PatternCtx) -> Label {
        if self.pattern_is_nullable(inner) {
            return self.compile_nullable_pattern(inner, ctx, SkipExit::Fail);
        }
        self.dispatch_pattern(inner, ctx)
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
            | IterationScope::ElementScopeByListExit { capture } => self.compile_iteration_element(
                inner,
                PatternCtx {
                    exit,
                    nav: Some(nav),
                    capture,
                    value: false,
                },
            ),
            IterationScope::RecordElement {
                element_type_id, ..
            } => self.compile_record_for_list(inner, exit, Some(nav), element_type_id),
            IterationScope::ElementScopeByIteration {
                element_type_id,
                capture,
            } => self.with_scope_if_present(element_type_id, |this| {
                this.compile_iteration_element(
                    inner,
                    PatternCtx {
                        exit,
                        nav: Some(nav),
                        capture,
                        value: false,
                    },
                )
            }),
            IterationScope::CaptureType { plan, .. } => self
                .capture_type(&plan, Some(nav), CaptureExits::Single(exit))
                .list_item(inner),
        }
    }
}

fn empty_path_control(instruction: &InstructionIR) -> bool {
    let InstructionIR::Match(matched) = instruction else {
        return false;
    };
    matches!(
        matched.nav,
        Nav::Epsilon | Nav::ChildlessSkipTrivia | Nav::ChildlessSkipExtras | Nav::ChildlessExact
    )
}
