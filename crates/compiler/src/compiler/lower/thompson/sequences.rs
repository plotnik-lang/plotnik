use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::bytecode::{EffectKind, Nav, SpanKind};
use crate::compiler::analyze::boundary::ExitPort;
use crate::compiler::analyze::boundary::{
    BoundaryRelation, BoundaryState, FirstClass, simple_boundary_outcomes,
};
use crate::compiler::analyze::result::CaptureMemberKind;
use crate::compiler::analyze::types::CaptureKind;
use crate::compiler::analyze::types::type_check::definition_value_root;
use crate::compiler::analyze::types::type_shape::PatternFlow;
use crate::compiler::ids::DefId;
use crate::compiler::lower::boundary::ExitMap;
use crate::compiler::lower::ir::{EffectIR, Label, MatchIR, NodeKindConstraint};
use crate::compiler::lower::spans::SpanBindingIR;
use crate::compiler::parse::ast::{self, Pattern, SeqItem};
use crate::core::NodeKindId;

use super::NfaBuilder;
use super::boundary::{
    EntryObligation, NavigationContract, next_boundary_state, trailing_childless_nav,
    trailing_up_nav,
};
use super::capture::{CaptureEffects, PatternCtx, first_unmatched_close};
use super::navigation::{is_down_nav, resumable_search_nav};
use super::scope::{ScopeCloseEffects, SkipExit};

fn trailing_anchor_follow_nav(items: &[SeqItem]) -> Option<Nav> {
    match items.last()? {
        SeqItem::Anchor(a) if a.is_exact() => Some(Nav::NextExact),
        SeqItem::Anchor(_) => Some(Nav::NextSkip),
        SeqItem::Pattern(_) => None,
    }
}

struct SequencePostEffects {
    item_post: Vec<EffectIR>,
    exit_post: Vec<EffectIR>,
}

fn split_sequence_tail_effects(post: Vec<EffectIR>) -> SequencePostEffects {
    let mut item_post = post;
    let exit_post = match first_unmatched_close(&item_post) {
        Some(split) => item_post.split_off(split),
        None => vec![],
    };

    SequencePostEffects {
        item_post,
        exit_post,
    }
}

pub(super) struct SeqItemsCtx<'a> {
    pub(super) items: &'a [SeqItem],
    pub(super) exit: Label,
    pub(super) node_final_exit: Option<Label>,
    pub(super) is_inside_node: bool,
    pub(super) first_nav: Option<Nav>,
    pub(super) capture: CaptureEffects,
    pub(super) skip_exit: Option<SkipExit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct BoundaryMemoKey {
    item_index: usize,
    port: ExitPort,
}

impl BoundaryMemoKey {
    fn new(item_index: usize, port: ExitPort) -> Self {
        Self { item_index, port }
    }
}

struct BoundaryItemsCtx<'a> {
    items: &'a [SeqItem],
    sequence_input: BoundaryState,
    entry: EntryObligation,
    targets: &'a ExitMap<Label>,
    external_retry: bool,
}

impl NfaBuilder<'_> {
    pub(super) fn compile_seq(&mut self, seq: &ast::SeqPattern, ctx: PatternCtx) -> Label {
        let PatternCtx {
            exit,
            nav: first_nav,
            capture,
            observe_value: _,
        } = ctx;
        let items: Vec<_> = seq.items().collect();
        if items.is_empty() {
            return exit;
        }

        let is_inside_node = matches!(
            first_nav,
            Some(Nav::Down | Nav::DownSkip | Nav::DownSkipExtras | Nav::DownExact)
        );

        self.compile_seq_items(SeqItemsCtx {
            items: &items,
            exit,
            node_final_exit: None,
            is_inside_node,
            first_nav,
            capture,
            skip_exit: None,
        })
    }

    pub(super) fn compile_seq_items(&mut self, ctx: SeqItemsCtx<'_>) -> Label {
        let SeqItemsCtx {
            items,
            exit,
            node_final_exit,
            is_inside_node,
            first_nav,
            capture,
            skip_exit,
        } = ctx;

        if self.boundary_sensitive_items(items) {
            assert!(
                self.boundary_items_supported(items),
                "an analyzed boundary-sensitive sequence must support multi-exit lowering"
            );
            return self.compile_boundary_items_root(
                items,
                exit,
                node_final_exit,
                EntryObligation::new(NavigationContract::from_nav(first_nav.unwrap_or(Nav::Down))),
                capture,
                skip_exit,
            );
        }

        let mut nav_modes = self
            .anchor_semantics
            .compute_nav_modes(items, is_inside_node);

        if nav_modes.is_empty() {
            return exit;
        }

        let first_pattern_nav = if let Some((_, first_mode)) = nav_modes.first_mut()
            && first_mode.is_none()
        {
            let nav = first_nav.or_else(|| is_inside_node.then_some(Nav::Down));
            *first_mode = nav;
            nav
        } else {
            nav_modes.first().and_then(|(_, n)| *n)
        };

        let first_is_skippable = nav_modes
            .first()
            .and_then(|(idx, _)| items[*idx].as_pattern())
            .is_some_and(|p| self.pattern_is_nullable(p));
        let first_positions = is_down_nav(first_pattern_nav)
            || matches!(first_pattern_nav, Some(Nav::Stay | Nav::StayExact));
        let needs_skip_exit =
            first_is_skippable && (skip_exit.is_some() || (first_positions && nav_modes.len() > 1));

        if needs_skip_exit {
            return self.compile_seq_items_with_skip_exit(
                &nav_modes,
                SeqItemsCtx {
                    items,
                    exit,
                    node_final_exit: None,
                    is_inside_node,
                    first_nav: first_pattern_nav,
                    capture,
                    skip_exit,
                },
            );
        }

        let last_is_skippable = nav_modes
            .last()
            .and_then(|(idx, _)| items[*idx].as_pattern())
            .is_some_and(|p| self.pattern_is_nullable(p));

        let mut current_exit = exit;
        let last_post = if last_is_skippable {
            let tail_effects = split_sequence_tail_effects(capture.post);
            if !tail_effects.exit_post.is_empty() {
                current_exit = self.emit_effects_epsilon(current_exit, tail_effects.exit_post);
            }
            tail_effects.item_post
        } else {
            capture.post
        };
        let count = nav_modes.len();
        let mut following_nav: Option<Nav> = trailing_anchor_follow_nav(items);
        for (i, (pattern_idx, nav_override)) in nav_modes.into_iter().rev().enumerate() {
            let pattern = items[pattern_idx]
                .as_pattern()
                .expect("nav_modes only contains pattern indices");

            let is_last_pattern = i == 0;
            let is_first_pattern = i == count - 1;

            let item_capture = CaptureEffects {
                pre: if is_first_pattern && !first_is_skippable {
                    capture.pre.clone()
                } else {
                    vec![]
                },
                post: if is_last_pattern {
                    last_post.clone()
                } else {
                    vec![]
                },
            };

            let followed_by_anchor = matches!(
                following_nav,
                Some(Nav::NextSkip | Nav::NextSkipExtras | Nav::NextExact)
            );
            let search_nav = resumable_search_nav(nav_override)
                .filter(|_| followed_by_anchor && !self.item_owns_iteration(pattern));

            current_exit = if let Some(nav) = search_nav {
                let pattern_ctx = PatternCtx {
                    exit: current_exit,
                    nav: Some(Nav::StayExact),
                    capture: item_capture,
                    observe_value: false,
                };
                let body = self.dispatch_pattern(pattern, pattern_ctx);
                self.emit_position_search(nav, body)
            } else {
                let pattern_ctx = PatternCtx {
                    exit: current_exit,
                    nav: nav_override,
                    capture: item_capture,
                    observe_value: false,
                };
                self.dispatch_pattern(pattern, pattern_ctx)
            };
            following_nav = nav_override;
        }
        if first_is_skippable {
            current_exit = self.wrap_entry_pre(current_exit, capture.pre);
        }
        current_exit
    }

    fn compile_boundary_items_root(
        &mut self,
        items: &[SeqItem],
        exit: Label,
        node_final_exit: Option<Label>,
        entry: EntryObligation,
        capture: CaptureEffects,
        skip_exit: Option<SkipExit>,
    ) -> Label {
        let relation = self.items_boundary_relation(items);
        let tail_effects = split_sequence_tail_effects(capture.post);
        let mut targets = ExitMap::new();
        for outcome in relation.outcomes(BoundaryState::START) {
            let target = if let Some(final_exit) = node_final_exit {
                if outcome.consumed {
                    if outcome.state.pending
                        == crate::compiler::analyze::boundary::PendingAnchor::None
                    {
                        Some(exit)
                    } else {
                        let target = self.fresh_label();
                        self.instructions.push(
                            crate::compiler::lower::ir::MatchIR::epsilon(target, final_exit)
                                .nav(trailing_up_nav(outcome.state, 1))
                                .into(),
                        );
                        Some(target)
                    }
                } else if let Some(nav) = trailing_childless_nav(outcome.state) {
                    let target = self.fresh_label();
                    self.instructions.push(
                        crate::compiler::lower::ir::MatchIR::epsilon(target, final_exit)
                            .nav(nav)
                            .into(),
                    );
                    Some(target)
                } else {
                    Some(final_exit)
                }
            } else if outcome.consumed {
                Some(exit)
            } else {
                match skip_exit.unwrap_or(SkipExit::To(exit)) {
                    SkipExit::To(exit) => Some(exit),
                    SkipExit::Fail => None,
                }
            };
            if let Some(target) = target {
                let target = if outcome.consumed {
                    let mut post = tail_effects.item_post.clone();
                    post.extend(tail_effects.exit_post.iter().cloned());
                    self.emit_effects_if_nonempty(target, post)
                } else {
                    let target =
                        self.emit_effects_if_nonempty(target, tail_effects.exit_post.clone());
                    self.emit_absence_for_skip_path(
                        target,
                        &CaptureEffects::new_post(tail_effects.item_post.clone()),
                    )
                };
                targets.insert(ExitPort::from_outcome(*outcome), target);
            }
        }
        if targets.is_empty() {
            let failure_exit = self.emit_effects_if_nonempty(exit, tail_effects.exit_post.clone());
            let failure_exit = self.emit_absence_for_skip_path(
                failure_exit,
                &CaptureEffects::new_post(tail_effects.item_post),
            );
            let failure = self.emit_boundary_failure(entry, failure_exit);
            return self.wrap_entry_pre(failure, capture.pre);
        }

        let mut memo = BTreeMap::new();
        let ctx = BoundaryItemsCtx {
            items,
            sequence_input: BoundaryState::START,
            entry,
            targets: &targets,
            external_retry: false,
        };
        let entry = self
            .compile_boundary_items_to(&ctx, 0, BoundaryState::START, false, &mut memo)
            .expect("a supported boundary sequence exposes an admitted operational exit");
        self.wrap_entry_pre(entry, capture.pre)
    }

    fn emit_boundary_failure(&mut self, obligation: EntryObligation, exit: Label) -> Label {
        let nav = match obligation.navigation().authored() {
            Nav::Stay | Nav::StayExact => Nav::StayExact,
            Nav::Next | Nav::NextSkip | Nav::NextSkipExtras | Nav::NextExact => Nav::NextExact,
            Nav::Down | Nav::DownSkip | Nav::DownSkipExtras | Nav::DownExact => Nav::DownExact,
            _ => unreachable!("entry obligations only stay, advance, or descend"),
        };
        let entry = self.fresh_label();
        // Tree-sitter's builtin ERROR node is always named. Constraining the
        // same kind as anonymous is therefore an internal never-match sentinel.
        self.instructions.push(
            MatchIR::epsilon(entry, exit)
                .nav(nav)
                .node_kind(NodeKindConstraint::Anonymous(Some(NodeKindId::ERROR)))
                .into(),
        );
        entry
    }

    fn compile_boundary_items_to(
        &mut self,
        ctx: &BoundaryItemsCtx<'_>,
        item_index: usize,
        state: BoundaryState,
        consumed: bool,
        memo: &mut BTreeMap<BoundaryMemoKey, Option<Label>>,
    ) -> Option<Label> {
        let port = ExitPort::from_state(state, consumed);
        let memo_key = BoundaryMemoKey::new(item_index, port);
        if let Some(result) = memo.get(&memo_key) {
            return *result;
        }

        let state = next_boundary_state(ctx.sequence_input, port);
        let Some(item) = ctx.items.get(item_index) else {
            let result = ctx.targets.get(port).copied();
            memo.insert(memo_key, result);
            return result;
        };

        let result = match item {
            SeqItem::Anchor(anchor) => {
                let pending = if anchor.is_exact() {
                    crate::compiler::analyze::boundary::PendingAnchor::Exact
                } else {
                    crate::compiler::analyze::boundary::PendingAnchor::Soft
                };
                let state = BoundaryState::new(state.previous, state.pending.tighten(pending));
                self.compile_boundary_items_to(ctx, item_index + 1, state, consumed, memo)
            }
            SeqItem::Pattern(pattern) => {
                let relation = self.pattern_boundary_relation(pattern);
                let outcomes: Vec<_> = relation.outcomes(state).iter().copied().collect();
                let mut pattern_targets = ExitMap::new();
                for outcome in &outcomes {
                    let Some(suffix) = self.compile_boundary_items_to(
                        ctx,
                        item_index + 1,
                        outcome.state,
                        consumed || outcome.consumed,
                        memo,
                    ) else {
                        continue;
                    };
                    pattern_targets.insert(ExitPort::from_outcome(*outcome), suffix);
                }
                let pattern_entry = if consumed {
                    EntryObligation::new(NavigationContract::from_nav(
                        ctx.entry.navigation().authored().sibling_continuation(),
                    ))
                } else {
                    ctx.entry
                };
                let suffix = &ctx.items[item_index + 1..];
                let retry_suffix = self.boundary_retry_needed(suffix)
                    || (ctx.external_retry && self.boundary_items_nullable(suffix));
                self.compile_boundary_pattern_to(
                    pattern,
                    state,
                    pattern_entry,
                    &pattern_targets,
                    retry_suffix,
                )
            }
        };
        memo.insert(memo_key, result);
        result
    }

    pub(super) fn compile_boundary_pattern_to(
        &mut self,
        pattern: &Pattern,
        input: BoundaryState,
        entry: EntryObligation,
        targets: &ExitMap<Label>,
        retry_suffix: bool,
    ) -> Option<Label> {
        if let Pattern::CapturedPattern(capture) = pattern {
            let relation = self.pattern_boundary_relation(pattern);
            let captures_reference = capture
                .inner()
                .is_some_and(|inner| matches!(inner, Pattern::DefRef(_)));
            let admits_consuming = relation.outcomes(input).iter().any(|outcome| {
                outcome.consumed && targets.get(ExitPort::from_outcome(*outcome)).is_some()
            });
            let admits_empty = relation.outcomes(input).iter().any(|outcome| {
                !outcome.consumed && targets.get(ExitPort::from_outcome(*outcome)).is_some()
            });
            if captures_reference
                || !simple_boundary_routes(relation, input)
                || (admits_empty && !admits_consuming)
            {
                return self.compile_boundary_capture(capture, input, entry, targets);
            }
        }

        if let Pattern::FieldPattern(field) = pattern
            && let Some(value) = field.value()
            && let Some(field_id) = self.resolve_field(field)
        {
            return self.compile_boundary_pattern_to(
                &value,
                input,
                entry.with_field(field_id),
                targets,
                retry_suffix,
            );
        }

        if let Pattern::SeqPattern(sequence) = pattern {
            let items: Vec<_> = sequence.items().collect();
            let mut memo = BTreeMap::new();
            let ctx = BoundaryItemsCtx {
                items: &items,
                sequence_input: input,
                entry,
                targets,
                external_retry: retry_suffix,
            };
            return self.compile_boundary_items_to(&ctx, 0, input, false, &mut memo);
        }

        if let Pattern::DefRef(reference) = pattern {
            let def_id = self.expect_reference_target(reference);
            return self.compile_boundary_ref_call(def_id, input, entry, targets);
        }

        if let Pattern::QuantifiedPattern(quantified) = pattern {
            let relation = self.pattern_boundary_relation(pattern);
            if !simple_boundary_routes(relation, input) {
                return self
                    .compile_boundary_no_value_quantifier(quantified, input, entry, targets);
            }
        }

        if let Pattern::Alternation(alternation) = pattern {
            let relation = self.pattern_boundary_relation(pattern);
            if !simple_boundary_routes(relation, input) {
                return self.compile_boundary_alternation(alternation, input, entry, targets);
            }
        }

        let relation = self.pattern_boundary_relation(pattern);
        let outcomes: Vec<_> = relation.outcomes(input).iter().copied().collect();
        if !simple_boundary_outcomes(&outcomes) {
            return None;
        }

        let mut consuming_target = None;
        let mut empty_target = None;
        let mut first_classes = BTreeSet::new();
        for outcome in outcomes {
            let target = targets.get(ExitPort::from_outcome(outcome)).copied();
            if outcome.consumed {
                first_classes.insert(outcome.first);
                if let Some(target) = target {
                    consuming_target = Some(target);
                }
            } else if let Some(target) = target {
                empty_target = Some(target);
            }
        }

        let Some(consuming_target) = consuming_target else {
            return empty_target;
        };
        let next_class = if first_classes.len() == 1 {
            *first_classes
                .first()
                .expect("one first-consumer class was measured")
        } else {
            FirstClass::Either
        };

        if let Some(field) = entry.field() {
            if self.pattern_is_nullable(pattern) {
                return None;
            }

            let nav = entry.resolve_nav(input, next_class);
            let exact_entry = EntryObligation::new(NavigationContract::from_nav(Nav::StayExact));
            let body =
                self.compile_boundary_pattern_to(pattern, input, exact_entry, targets, false)?;
            let search = resumable_search_nav(Some(nav)).filter(|_| {
                retry_suffix
                    && !self.pattern_is_nullable(pattern)
                    && !self.item_owns_iteration(pattern)
            });
            if let Some(search) = search {
                return Some(self.emit_position_search_with_field(search, field, body));
            }

            let wrapper = self.fresh_label();
            self.instructions.push(
                crate::compiler::lower::ir::MatchIR::epsilon(wrapper, body)
                    .nav(nav)
                    .node_field(field)
                    .into(),
            );
            return Some(wrapper);
        }

        let nav = entry.resolve_nav(input, next_class);
        let search_nav = resumable_search_nav(Some(nav)).filter(|_| {
            retry_suffix && !self.pattern_is_nullable(pattern) && !self.item_owns_iteration(pattern)
        });
        let pattern_ctx = PatternCtx {
            exit: consuming_target,
            nav: Some(search_nav.map_or(nav, |_| Nav::StayExact)),
            capture: CaptureEffects::default(),
            observe_value: false,
        };

        let pattern_entry = if relation
            .outcomes(input)
            .iter()
            .any(|outcome| !outcome.consumed)
        {
            let skip_exit = empty_target.map_or(SkipExit::Fail, SkipExit::To);
            self.compile_nullable_pattern(pattern, pattern_ctx, skip_exit)
        } else {
            self.dispatch_pattern(pattern, pattern_ctx)
        };
        Some(search_nav.map_or(pattern_entry, |nav| {
            self.emit_position_search(nav, pattern_entry)
        }))
    }

    fn compile_boundary_capture(
        &mut self,
        captured_pattern: &ast::CapturedPattern,
        input: BoundaryState,
        entry: EntryObligation,
        targets: &ExitMap<Label>,
    ) -> Option<Label> {
        let inner = captured_pattern
            .inner()
            .expect("validated capture must have an inner pattern");
        if captured_pattern.capture().is_discard() || self.is_suppressed() {
            return self.with_suppression(|this| {
                this.compile_boundary_pattern_to(&inner, input, entry, targets, false)
            });
        }

        let pattern = Pattern::CapturedPattern(captured_pattern.clone());
        let fact = self
            .ctx
            .analysis
            .type_analysis
            .expect_capture_fact(&pattern);
        let capture_type_plan = fact.built_in_plan().map(|(_, plan)| plan.clone());
        let mechanism = fact.kind();
        if let Some(plan) = capture_type_plan {
            return self.compile_boundary_capture_type(
                captured_pattern,
                &plan,
                input,
                entry,
                targets,
            );
        }
        let mut capture_effects = self.build_capture_effects(captured_pattern, mechanism);
        let mut selection = Vec::new();
        capture_effects.retain(|effect| {
            if effect.kind() == EffectKind::RecordSet {
                selection.push(effect.clone());
                false
            } else {
                true
            }
        });
        let value_entry = (|| {
            if let Pattern::QuantifiedPattern(quantified) = &inner {
                return match mechanism {
                    CaptureKind::List => self.compile_boundary_list_capture(
                        quantified,
                        input,
                        entry,
                        targets,
                        capture_effects,
                    ),
                    CaptureKind::Node
                    | CaptureKind::Record
                    | CaptureKind::Ref
                    | CaptureKind::PendingValue => self.compile_boundary_optional_capture(
                        quantified,
                        input,
                        entry,
                        targets,
                        capture_effects,
                    ),
                };
            }

            if mechanism == CaptureKind::Ref
                && let Pattern::DefRef(reference) = &inner
            {
                let def_id = self.expect_reference_target(reference);
                return self.compile_boundary_captured_ref_call(
                    def_id,
                    input,
                    entry,
                    targets,
                    &capture_effects,
                );
            }

            match mechanism {
                CaptureKind::Record => {
                    let scope_type_id = self
                        .ctx
                        .analysis
                        .type_analysis
                        .expect_pattern_flow(&inner)
                        .type_id();
                    let mut inner_targets = ExitMap::new();
                    for (port, &target) in targets.iter() {
                        let close = self.emit_record_close_with_effects(
                            ScopeCloseEffects {
                                leading: &[],
                                capture: &capture_effects,
                                outer: &[],
                            },
                            target,
                        );
                        inner_targets.insert(port, close);
                    }
                    let inner_entry = self.with_scope_if_present(scope_type_id, |this| {
                        this.compile_boundary_pattern_to(
                            &inner,
                            input,
                            entry,
                            &inner_targets,
                            false,
                        )
                    })?;
                    Some(self.emit_record_open_with_pre(inner_entry, vec![]))
                }
                CaptureKind::Node | CaptureKind::Ref | CaptureKind::PendingValue => {
                    let capture = CaptureEffects::new_post(capture_effects.clone());
                    let mut inner_targets = ExitMap::new();
                    for (port, &target) in targets.iter() {
                        let target = if port.consumed() || mechanism == CaptureKind::PendingValue {
                            self.emit_effects_if_nonempty(target, capture_effects.clone())
                        } else {
                            self.emit_absence_for_skip_path(target, &capture)
                        };
                        inner_targets.insert(port, target);
                    }
                    if mechanism == CaptureKind::PendingValue
                        && let Pattern::Alternation(alternation) = &inner
                        && matches!(
                            self.ctx.analysis.type_analysis.expect_pattern_flow(&inner),
                            PatternFlow::Value(_)
                        )
                    {
                        return self.compile_boundary_labeled_alternation(
                            alternation,
                            input,
                            entry,
                            &inner_targets,
                        );
                    }
                    self.compile_boundary_pattern_to(&inner, input, entry, &inner_targets, false)
                }
                CaptureKind::List => {
                    unreachable!("a list capture is backed by a quantified pattern")
                }
            }
        })()?;
        Some(self.wrap_entry_pre(value_entry, selection))
    }

    fn compile_boundary_alternation(
        &mut self,
        alternation: &ast::AlternationPattern,
        input: BoundaryState,
        entry: EntryObligation,
        targets: &ExitMap<Label>,
    ) -> Option<Label> {
        let alternation_pattern = Pattern::Alternation(alternation.clone());
        let relation = self.pattern_boundary_relation(&alternation_pattern);
        let first_classes: BTreeSet<_> = relation
            .outcomes(input)
            .iter()
            .filter(|outcome| {
                outcome.consumed && targets.get(ExitPort::from_outcome(**outcome)).is_some()
            })
            .map(|outcome| outcome.first)
            .collect();
        let search_nav = if first_classes.len() == 1 {
            let next_class = *first_classes
                .first()
                .expect("one first-consumer class was measured");
            resumable_search_nav(Some(entry.resolve_nav(input, next_class)))
        } else {
            None
        };
        let consuming_entry = search_nav.map_or(entry, |_| {
            EntryObligation::new(NavigationContract::from_nav(Nav::StayExact))
        });

        let mut consuming_entries = Vec::new();
        let mut empty_entries = Vec::new();
        let alternation_type_id = if self.is_suppressed() {
            None
        } else {
            match self
                .ctx
                .analysis
                .type_analysis
                .expect_pattern_flow(&alternation_pattern)
            {
                PatternFlow::Fields(id) => Some(id),
                PatternFlow::NoValue | PatternFlow::Value(_) => None,
            }
        };
        let merged_fields =
            alternation_type_id.map(|id| self.ctx.analysis.type_analysis.expect_record_fields(*id));
        let field_completions = alternation_type_id.map(|_| {
            self.ctx
                .analysis
                .type_analysis
                .expect_field_completions(&alternation_pattern)
        });

        for alternative in alternation.alternatives() {
            let Some(body) = alternative.body() else {
                continue;
            };
            let body_relation = self.pattern_boundary_relation(&body).clone();
            let mut consuming_targets = ExitMap::new();
            let mut empty_targets = ExitMap::new();
            let alternative_span = self.span_id(alternative.syntax(), SpanKind::Alternative);
            for outcome in body_relation.outcomes(input) {
                let port = ExitPort::from_outcome(*outcome);
                if let Some(&target) = targets.get(port) {
                    let target = if let Some(span) = alternative_span {
                        self.emit_effects_if_nonempty(target, vec![EffectIR::span_end(span.0)])
                    } else {
                        target
                    };
                    if outcome.consumed {
                        consuming_targets.insert(port, target);
                    } else {
                        empty_targets.insert(port, target);
                    }
                }
            }
            if consuming_targets.is_empty() && empty_targets.is_empty() {
                continue;
            }

            let mut pre = Vec::new();
            if let Some(fields) = merged_fields {
                let provided: HashSet<_> =
                    match &self.ctx.analysis.type_analysis.expect_pattern_flow(&body) {
                        PatternFlow::Fields(id) => self
                            .ctx
                            .analysis
                            .type_analysis
                            .expect_record_fields(*id)
                            .keys()
                            .copied()
                            .collect(),
                        PatternFlow::NoValue | PatternFlow::Value(_) => HashSet::new(),
                    };
                pre.extend(self.merged_field_completion_effects(
                    field_completions.expect("merged alternation has field completions"),
                    fields,
                    &provided,
                ));
            }
            if let Some(span) = alternative_span {
                pre.push(EffectIR::span_start(span.0));
            }
            if !consuming_targets.is_empty()
                && let Some(body_entry) = self.compile_boundary_pattern_to(
                    &body,
                    input,
                    consuming_entry,
                    &consuming_targets,
                    false,
                )
            {
                consuming_entries.push(self.wrap_entry_pre(body_entry, pre.clone()));
            }
            if !empty_targets.is_empty()
                && let Some(body_entry) =
                    self.compile_boundary_pattern_to(&body, input, entry, &empty_targets, false)
            {
                empty_entries.push(self.wrap_entry_pre(body_entry, pre));
            }
        }

        self.assemble_boundary_alternatives(
            consuming_entries,
            empty_entries,
            search_nav,
            entry.field(),
        )
    }

    pub(super) fn compile_boundary_labeled_alternation(
        &mut self,
        alternation: &ast::AlternationPattern,
        input: BoundaryState,
        entry: EntryObligation,
        targets: &ExitMap<Label>,
    ) -> Option<Label> {
        let alternation_pattern = Pattern::Alternation(alternation.clone());
        let variant_type_id = self
            .ctx
            .analysis
            .type_analysis
            .expect_pattern_flow(&alternation_pattern)
            .type_id()
            .expect("an observed labeled alternation produces a variant type");
        let relation = self.pattern_boundary_relation(&alternation_pattern);
        let first_classes: BTreeSet<_> = relation
            .outcomes(input)
            .iter()
            .filter(|outcome| {
                outcome.consumed && targets.get(ExitPort::from_outcome(**outcome)).is_some()
            })
            .map(|outcome| outcome.first)
            .collect();
        let search_nav = if first_classes.len() == 1 {
            let next_class = *first_classes
                .first()
                .expect("one first-consumer class was measured");
            resumable_search_nav(Some(entry.resolve_nav(input, next_class)))
        } else {
            None
        };
        let consuming_entry = search_nav.map_or(entry, |_| {
            EntryObligation::new(NavigationContract::from_nav(Nav::StayExact))
        });

        let mut consuming_entries = Vec::new();
        let mut empty_entries = Vec::new();
        for alternative in alternation.alternatives() {
            let Some(body) = alternative.body() else {
                continue;
            };
            let label = alternative
                .label()
                .expect("observed labeled alternation has labeled alternatives");
            let case_name = self
                .ctx
                .analysis
                .interner
                .get(label.text())
                .expect("labeled alternative name is interned");
            let case = self
                .ctx
                .result
                .layout()
                .member_id(variant_type_id, case_name)
                .expect("case exists for labeled alternative");
            let CaptureMemberKind::Case(payload) =
                self.ctx.result.layout().expect_member(case).kind
            else {
                unreachable!("a variant result scope contains only cases")
            };
            let alternative_span = self.span_id(alternative.syntax(), SpanKind::Alternative);
            if let Some(span) = alternative_span {
                self.bind_span(span, SpanBindingIR::Member(case));
            }

            let body_relation = self.pattern_boundary_relation(&body).clone();
            let mut consuming_targets = ExitMap::new();
            let mut empty_targets = ExitMap::new();
            for outcome in body_relation.outcomes(input) {
                let port = ExitPort::from_outcome(*outcome);
                let Some(&target) = targets.get(port) else {
                    continue;
                };
                let mut close = vec![EffectIR::end_variant()];
                if let Some(span) = alternative_span {
                    close.push(EffectIR::span_end(span.0));
                }
                let target = self.emit_effects_if_nonempty(target, close);
                if outcome.consumed {
                    consuming_targets.insert(port, target);
                } else {
                    empty_targets.insert(port, target);
                }
            }
            if consuming_targets.is_empty() && empty_targets.is_empty() {
                continue;
            }

            let mut pre = Vec::new();
            if let Some(span) = alternative_span {
                pre.push(EffectIR::span_start(span.0));
            }
            pre.push(EffectIR::with_member(EffectKind::VariantOpen, case));
            if !consuming_targets.is_empty()
                && let Some(body_entry) = self.with_scope_if_present(payload.type_id(), |this| {
                    this.compile_boundary_pattern_to(
                        &body,
                        input,
                        consuming_entry,
                        &consuming_targets,
                        false,
                    )
                })
            {
                consuming_entries.push(self.wrap_entry_pre(body_entry, pre.clone()));
            }
            if !empty_targets.is_empty()
                && let Some(body_entry) = self.with_scope_if_present(payload.type_id(), |this| {
                    this.compile_boundary_pattern_to(&body, input, entry, &empty_targets, false)
                })
            {
                empty_entries.push(self.wrap_entry_pre(body_entry, pre));
            }
        }

        self.assemble_boundary_alternatives(
            consuming_entries,
            empty_entries,
            search_nav,
            entry.field(),
        )
    }

    fn assemble_boundary_alternatives(
        &mut self,
        consuming: Vec<Label>,
        empty: Vec<Label>,
        search_nav: Option<Nav>,
        field: Option<crate::core::NodeFieldId>,
    ) -> Option<Label> {
        let consuming = match consuming.as_slice() {
            [] => None,
            [only] => Some(*only),
            _ => {
                let fork = self.fresh_label();
                self.emit_epsilon(fork, consuming);
                Some(fork)
            }
        };
        let consuming = match (consuming, search_nav) {
            (Some(body), Some(nav)) => Some(if let Some(field) = field {
                self.emit_position_search_with_field(nav, field, body)
            } else {
                self.emit_position_search(nav, body)
            }),
            (entry, None) => entry,
            (None, Some(_)) => None,
        };

        let alternatives: Vec<_> = consuming.into_iter().chain(empty).collect();
        match alternatives.as_slice() {
            [] => None,
            [only] => Some(*only),
            _ => {
                let fork = self.fresh_label();
                self.emit_epsilon(fork, alternatives);
                Some(fork)
            }
        }
    }

    fn boundary_sensitive_items(&self, items: &[SeqItem]) -> bool {
        let mut visited = HashSet::new();
        self.boundary_sensitive_items_with_visited(items, &mut visited)
    }

    fn boundary_retry_needed(&self, items: &[SeqItem]) -> bool {
        let mut visited = HashSet::new();
        for item in items {
            match item {
                SeqItem::Anchor(_) => return true,
                SeqItem::Pattern(pattern) => {
                    if self.boundary_sensitive_pattern(pattern, &mut visited) {
                        return true;
                    }
                    if !self.pattern_is_nullable(pattern) {
                        return false;
                    }
                }
            }
        }
        false
    }

    fn boundary_items_nullable(&self, items: &[SeqItem]) -> bool {
        items.iter().all(|item| match item {
            SeqItem::Anchor(_) => true,
            SeqItem::Pattern(pattern) => self.pattern_is_nullable(pattern),
        })
    }

    fn boundary_sensitive_items_with_visited(
        &self,
        items: &[SeqItem],
        visited: &mut HashSet<DefId>,
    ) -> bool {
        items.iter().any(|item| match item {
            SeqItem::Anchor(_) => true,
            SeqItem::Pattern(pattern) => self.boundary_sensitive_pattern(pattern, visited),
        })
    }

    fn boundary_sensitive_pattern(&self, pattern: &Pattern, visited: &mut HashSet<DefId>) -> bool {
        match pattern {
            Pattern::SeqPattern(sequence) => {
                let items: Vec<_> = sequence.items().collect();
                self.boundary_sensitive_items_with_visited(&items, visited)
            }
            Pattern::DefRef(reference) => {
                let def_id = self.expect_reference_target(reference);
                if !visited.insert(def_id) {
                    return false;
                }
                let body = self.ctx.analysis.definitions.definition(def_id).body();
                let result = self.boundary_sensitive_pattern(body, visited);
                visited.remove(&def_id);
                result
            }
            Pattern::CapturedPattern(capture) => capture
                .inner()
                .is_some_and(|inner| self.boundary_sensitive_pattern(&inner, visited)),
            Pattern::QuantifiedPattern(quantified) => quantified
                .inner()
                .is_some_and(|inner| self.boundary_sensitive_pattern(&inner, visited)),
            Pattern::FieldPattern(field) => field
                .value()
                .is_some_and(|value| self.boundary_sensitive_pattern(&value, visited)),
            Pattern::Alternation(alternation) => alternation
                .patterns()
                .any(|body| self.boundary_sensitive_pattern(&body, visited)),
            Pattern::NamedNodePattern(_)
            | Pattern::AnonymousNodePattern(_)
            | Pattern::NodeWildcard(_) => false,
        }
    }

    fn boundary_items_supported(&self, items: &[SeqItem]) -> bool {
        let mut visited = HashSet::new();
        self.boundary_items_supported_with_visited(items, &mut visited)
    }

    fn boundary_items_supported_with_visited(
        &self,
        items: &[SeqItem],
        visited: &mut HashSet<DefId>,
    ) -> bool {
        items.iter().all(|item| match item {
            SeqItem::Anchor(_) => true,
            SeqItem::Pattern(pattern) => self.boundary_pattern_supported(pattern, visited),
        })
    }

    fn boundary_pattern_supported(&self, pattern: &Pattern, visited: &mut HashSet<DefId>) -> bool {
        match pattern {
            Pattern::SeqPattern(sequence) => {
                let items: Vec<_> = sequence.items().collect();
                self.boundary_items_supported_with_visited(&items, visited)
            }
            Pattern::DefRef(reference) => {
                let def_id = self.expect_reference_target(reference);
                let relation = self.definition_boundary_relation(def_id);
                if BoundaryState::all().all(|input| simple_boundary_routes(relation, input)) {
                    return true;
                }
                if !visited.insert(def_id) {
                    return true;
                }
                let body = self.ctx.analysis.definitions.definition(def_id).body();
                let supported = if definition_value_root(body) {
                    self.boundary_definition_value_supported(body, visited)
                } else {
                    self.boundary_pattern_supported(body, visited)
                };
                visited.remove(&def_id);
                supported
            }
            Pattern::Alternation(alternation) => {
                let relation = self.pattern_boundary_relation(pattern);
                if BoundaryState::all().all(|input| simple_boundary_routes(relation, input)) {
                    return true;
                }
                alternation
                    .patterns()
                    .all(|body| self.boundary_pattern_supported(&body, visited))
            }
            Pattern::QuantifiedPattern(quantified) => {
                let relation = self.pattern_boundary_relation(pattern);
                if BoundaryState::all().all(|input| simple_boundary_routes(relation, input)) {
                    return true;
                }
                if !self
                    .ctx
                    .analysis
                    .type_analysis
                    .expect_pattern_flow(pattern)
                    .is_no_value()
                {
                    return false;
                }
                quantified
                    .inner()
                    .is_some_and(|inner| self.boundary_pattern_supported(&inner, visited))
            }
            Pattern::CapturedPattern(captured_pattern) => {
                let relation = self.pattern_boundary_relation(pattern);
                if BoundaryState::all().all(|input| simple_boundary_routes(relation, input)) {
                    return true;
                }
                let Some(inner) = captured_pattern.inner() else {
                    return true;
                };
                if captured_pattern.capture().is_discard() || self.is_suppressed() {
                    return self.boundary_pattern_supported(&inner, visited);
                }
                let fact = self.ctx.analysis.type_analysis.expect_capture_fact(pattern);
                if let Some((_, plan)) = fact.built_in_plan() {
                    return self.boundary_capture_type_supported(&inner, plan)
                        && self.boundary_capture_type_pattern_supported(&inner, plan, visited);
                }
                if let Pattern::QuantifiedPattern(quantified) = &inner {
                    return quantified
                        .inner()
                        .is_some_and(|body| self.boundary_pattern_supported(&body, visited));
                }
                self.boundary_pattern_supported(&inner, visited)
            }
            Pattern::FieldPattern(field) => field.value().is_some_and(|value| {
                !self.pattern_is_nullable(&value)
                    && self.boundary_pattern_supported(&value, visited)
            }),
            _ => {
                let relation = self.pattern_boundary_relation(pattern);
                BoundaryState::all().all(|input| simple_boundary_routes(relation, input))
            }
        }
    }

    fn boundary_definition_value_supported(
        &self,
        pattern: &Pattern,
        visited: &mut HashSet<DefId>,
    ) -> bool {
        match pattern {
            Pattern::FieldPattern(field) => field
                .value()
                .is_some_and(|value| self.boundary_definition_value_supported(&value, visited)),
            Pattern::QuantifiedPattern(quantified) => quantified
                .inner()
                .is_some_and(|inner| self.boundary_pattern_supported(&inner, visited)),
            Pattern::Alternation(alternation) => alternation
                .patterns()
                .all(|body| self.boundary_pattern_supported(&body, visited)),
            _ => unreachable!("definition value roots are labeled alternations or quantifiers"),
        }
    }

    fn boundary_capture_type_pattern_supported(
        &self,
        pattern: &Pattern,
        plan: &crate::compiler::analyze::types::CaptureTypePlan,
        visited: &mut HashSet<DefId>,
    ) -> bool {
        use crate::compiler::analyze::types::CaptureTypePlanKind;

        match plan.kind() {
            CaptureTypePlanKind::TextTerminal { .. } | CaptureTypePlanKind::BoolTerminal { .. } => {
                self.boundary_pattern_supported(pattern, visited)
            }
            CaptureTypePlanKind::Option { inner, .. }
            | CaptureTypePlanKind::List { element: inner } => {
                let Pattern::QuantifiedPattern(quantified) = pattern else {
                    return false;
                };
                let Some(body) = quantified.inner() else {
                    return false;
                };
                self.boundary_capture_type_pattern_supported(&body, inner, visited)
            }
        }
    }

    fn compile_seq_items_with_skip_exit(
        &mut self,
        nav_modes: &[(usize, Option<Nav>)],
        ctx: SeqItemsCtx<'_>,
    ) -> Label {
        let SeqItemsCtx {
            items,
            exit,
            node_final_exit: _,
            is_inside_node,
            first_nav,
            capture,
            skip_exit: caller_skip_exit,
        } = ctx;
        let first_pattern_idx = nav_modes[0].0;
        let first_pattern = items[first_pattern_idx]
            .as_pattern()
            .expect("first item must be pattern");

        let tail_effects = split_sequence_tail_effects(capture.post);
        let exit_post = tail_effects.exit_post;
        let exit = self.emit_effects_if_nonempty(exit, exit_post.clone());
        let caller_skip_exit = match caller_skip_exit {
            Some(SkipExit::To(skip)) => {
                Some(SkipExit::To(self.emit_effects_if_nonempty(skip, exit_post)))
            }
            other => other,
        };

        let (skip_exit, match_exit, first_post) = if nav_modes.len() < 2 {
            (
                caller_skip_exit.unwrap_or(SkipExit::To(exit)),
                exit,
                CaptureEffects::new_post(tail_effects.item_post),
            )
        } else {
            let cont = CaptureEffects::new_post(tail_effects.item_post);
            let mut skip_rest = Vec::with_capacity(items.len() - 1);
            skip_rest.extend_from_slice(&items[..first_pattern_idx]);
            skip_rest.extend_from_slice(&items[first_pattern_idx + 1..]);
            let skip = self.compile_seq_items(SeqItemsCtx {
                items: &skip_rest,
                exit,
                node_final_exit: None,
                is_inside_node,
                first_nav,
                capture: cont.clone(),
                skip_exit: caller_skip_exit,
            });

            let follower_idx = nav_modes[1].0;
            let match_rest = &items[follower_idx..];
            let mtch = self.compile_seq_items(SeqItemsCtx {
                items: match_rest,
                exit,
                node_final_exit: None,
                is_inside_node,
                first_nav: nav_modes[1].1,
                capture: cont,
                skip_exit: None,
            });
            (SkipExit::To(skip), mtch, CaptureEffects::default())
        };

        let pattern_ctx = PatternCtx {
            exit: match_exit,
            nav: first_nav,
            capture: first_post,
            observe_value: false,
        };
        let entry = self.compile_nullable_pattern(first_pattern, pattern_ctx, skip_exit);

        self.wrap_entry_pre(entry, capture.pre)
    }
}

fn simple_boundary_routes(relation: &BoundaryRelation, input: BoundaryState) -> bool {
    let outcomes: Vec<_> = relation.outcomes(input).iter().copied().collect();
    simple_boundary_outcomes(&outcomes)
}
