use std::collections::{BTreeMap, HashSet};

use crate::bytecode::{EffectKind, Nav, SpanKind};
use crate::compiler::analyze::types::TypeShape;
use crate::compiler::analyze::types::type_shape::FieldInfo;
use crate::compiler::analyze::types::type_shape::PatternFlow;
use crate::compiler::analyze::types::{FieldCompletion, FieldCompletions};
use crate::compiler::ids::TypeId;
use crate::compiler::lower::ir::{
    EffectIR, InstructionIR, Label, MatchIR, MemberRef, NodeKindConstraint,
};
use crate::compiler::lower::spans::SpanBindingIR;
use crate::compiler::parse::ast::{self, Pattern};
use crate::core::Symbol;

use super::NfaBuilder;
use super::capture::{CaptureEffects, PatternCtx};
use super::navigation::{AnchorSemantics, pattern_owns_iteration, resumable_search_nav};
use super::scope::SkipExit;

/// The alternation's resumable search nav (from [`resumable_search_nav`]), kept
/// distinct from an alternative's `first_nav` so the adjacent `Option<Nav>` inputs
/// to [`nav_for_alternative`] cannot be transposed. `Some` means the alternation
/// owns the retry wrapper and each alternative matches exactly at the candidate
/// (`StayExact`).
#[derive(Clone, Copy)]
struct AltSearchNav(Option<Nav>);

struct AlternativeRouting {
    alternative_named: Vec<bool>,
    named_exit: Option<Label>,
}

impl AlternativeRouting {
    fn alternative_exit(&self, alternative_idx: usize, default_exit: Label) -> Label {
        match self.named_exit {
            Some(skip) if self.alternative_named[alternative_idx] => skip,
            _ => default_exit,
        }
    }
}

fn exact_nav_for_alternative(first_nav: Option<Nav>, search_nav: AltSearchNav) -> Option<Nav> {
    if search_nav.0.is_some() {
        return Some(Nav::StayExact);
    }

    let nav = match first_nav {
        None => Nav::StayExact,
        Some(
            nav @ (Nav::DownSkip
            | Nav::DownSkipExtras
            | Nav::NextSkip
            | Nav::NextSkipExtras
            | Nav::UpSkipTrivia(_)
            | Nav::UpSkipExtras(_)),
        ) => nav,
        Some(nav) => nav.to_exact(),
    };
    Some(nav)
}

fn nav_for_alternative(
    first_nav: Option<Nav>,
    search_nav: AltSearchNav,
    body: &Pattern,
    anchor_semantics: &AnchorSemantics<'_>,
) -> Option<Nav> {
    let nav = exact_nav_for_alternative(first_nav, search_nav)?;

    if !anchor_semantics.pattern_may_match_anonymous(Some(body)) {
        return Some(nav);
    }

    Some(match nav {
        Nav::DownSkip => Nav::DownSkipExtras,
        Nav::NextSkip => Nav::NextSkipExtras,
        nav => nav,
    })
}

impl NfaBuilder<'_> {
    /// Clone a soft-anchor follower into a named-node-only retry entry.
    ///
    /// Sequence lowering has already compiled the follower after the alternation's
    /// `exit`. Soft anchors normally conservatively skip extras only
    /// (`NextSkipExtras`) because an anonymous-token left side must not skip
    /// anonymous tokens before the follower. When an alternative definitely matched a
    /// named node, the soft-anchor rule allows anonymous-token skipping too, so a
    /// `NextSkip` clone of that follower preserves the intended soft-anchor
    /// semantics for named alternatives without weakening anonymous alternatives.
    ///
    /// The clone is intentionally narrow: it reuses the already-compiled follower's
    /// successor, effects, predicate, field constraints, etc., so only navigation
    /// changes. That keeps the alternative-specific tweak local and avoids recompiling a
    /// sibling suffix with duplicated effects. The clone is appended after its
    /// successor has already been emitted; label references are symbolic IR, so
    /// the order is irrelevant until packing.
    ///
    /// A captured/tagged alternation does not exit straight into the follower:
    /// capture lowering interposes effect epsilons (`Set`, scope closes) between
    /// alternation exit and follower (#472). The walk below sees through that
    /// chain and clones it along with the follower, so each alternative runs the
    /// chain's effects exactly once — via the named twin or the conservative
    /// original, never both.
    ///
    /// Returns `None` — caller stays conservative — unless the chain is
    /// single-successor epsilons ending at a `Match` carrying `NextSkipExtras` on
    /// a `Named` node, the one shape where the upgrade is both safe and needed.
    /// The `Named` check matters because `NextSkipExtras` is ambiguous: it also
    /// appears when the *follower* may match anonymous nodes, and then extras-only
    /// skipping is correct even after a named alternative. Anonymous/`_` followers fail
    /// that check; a ref follower (`Call`) is skipped because the IR alone cannot
    /// prove the callee never matches an anonymous node.
    fn clone_named_follower_skip_entry(&mut self, exit: Label) -> Option<Label> {
        let mut chain: Vec<MatchIR> = Vec::new();
        let mut seen = HashSet::new();
        let mut cursor = exit;
        let mut twin = loop {
            if !seen.insert(cursor) {
                return None;
            }
            let InstructionIR::Match(m) = self.instructions.iter().find(|i| i.label() == cursor)?
            else {
                return None;
            };
            if !m.is_epsilon() {
                break m.clone();
            }
            let &[next] = m.successors.as_slice() else {
                return None;
            };
            chain.push(m.clone());
            cursor = next;
        };

        if twin.nav != Nav::NextSkipExtras
            || !matches!(twin.node_kind, NodeKindConstraint::Named(_))
        {
            return None;
        }

        twin.label = self.fresh_label();
        twin.nav = Nav::NextSkip;
        let mut entry = twin.label;
        self.instructions.push(twin.into());
        for mut eps in chain.into_iter().rev() {
            eps.label = self.fresh_label();
            eps.successors = vec![entry];
            entry = eps.label;
            self.instructions.push(eps.into());
        }
        Some(entry)
    }

    /// Per-alternative "named" flags plus the soft-skip follower twin. An alternative
    /// is "named" (eligible for the twin) when it cannot
    /// match an anonymous node and does not own its own iteration. A quantified
    /// alternative's zero-match path leaves no named node on the anchor's left, so the
    /// soft-skip upgrade is unsound there. The anonymity test covers the whole alternative,
    /// matching `nav_for_alternative`'s before-anchor classification. The twin is a
    /// `NextSkip` clone of a conservative (`NextSkipExtras`) soft follower, worth
    /// cloning only when at least one alternative is itself named.
    fn alternative_routing(
        &mut self,
        alternatives: &[ast::Alternative],
        exit: Label,
    ) -> AlternativeRouting {
        let alternative_named: Vec<bool> = {
            let anchor_semantics = &self.anchor_semantics;
            alternatives
                .iter()
                .map(|b| {
                    b.body().is_some_and(|body| {
                        !pattern_owns_iteration(&body)
                            && !anchor_semantics.pattern_may_match_anonymous(Some(&body))
                    })
                })
                .collect()
        };

        let named_exit = alternative_named
            .iter()
            .any(|&named| named)
            .then(|| self.clone_named_follower_skip_entry(exit))
            .flatten();

        AlternativeRouting {
            alternative_named,
            named_exit,
        }
    }

    /// A resumable search nav (`Down`/`Next`/`Stay`) gets one position-search retry
    /// wrapper around the fanned-in alternatives; otherwise each alternative performed
    /// its own exact navigation.
    ///
    /// `zero_width` holds the lifted zero-width continuations of nullable
    /// alternatives (see [`compile_unlabeled_alternatives`](Self::compile_unlabeled_alternatives)).
    /// They sit outside the position search — a zero-width outcome needs no
    /// candidate node — and after it: consuming matches, at any candidate and
    /// in any alternative, are preferred over a zero-width one.
    fn assemble_alternatives(
        &mut self,
        successors: Vec<Label>,
        zero_width: Vec<Label>,
        search_nav: Option<Nav>,
        exit: Label,
    ) -> Label {
        if successors.is_empty() && zero_width.is_empty() {
            return exit;
        }

        let real_entry = if successors.is_empty() {
            None
        } else {
            let alt_entry = if successors.len() == 1 {
                successors[0]
            } else {
                let entry = self.fresh_label();
                self.emit_epsilon(entry, successors);
                entry
            };
            Some(match search_nav {
                Some(nav) => self.emit_position_search(nav, alt_entry),
                None => alt_entry,
            })
        };

        let mut alternatives: Vec<Label> = real_entry.into_iter().chain(zero_width).collect();
        if alternatives.len() == 1 {
            return alternatives.remove(0);
        }
        let entry = self.fresh_label();
        self.emit_epsilon(entry, alternatives);
        entry
    }

    /// An untagged alternation merges each alternative's fields into one struct.
    pub(super) fn compile_unlabeled_alternation(
        &mut self,
        alternation: &ast::AlternationPattern,
        ctx: PatternCtx,
    ) -> Label {
        let skip_exit = SkipExit::To(ctx.exit);
        self.compile_unlabeled_alternation_with_exits(alternation, ctx, skip_exit)
    }

    /// [`compile_unlabeled_alternation`](Self::compile_unlabeled_alternation) with a distinct zero-width
    /// continuation (a skippable sequence item, or a pruned iteration element).
    pub(super) fn compile_unlabeled_alternation_with_exits(
        &mut self,
        alternation: &ast::AlternationPattern,
        ctx: PatternCtx,
        skip_exit: SkipExit,
    ) -> Label {
        let alternatives: Vec<_> = alternation.alternatives().collect();
        self.compile_unlabeled_alternatives(
            &Pattern::Alternation(alternation.clone()),
            &alternatives,
            ctx,
            skip_exit,
        )
    }

    /// Lower an alternation without variant tagging. `alternation` is the pattern whose inferred result carries
    /// the merged output struct.
    ///
    /// A nullable alternative compiles pruned ([`SkipExit::Fail`]) so its body only
    /// matches by consuming; its zero-width outcome is lifted to one shared
    /// alternative after the candidate search — a pure-effect epsilon that
    /// defaults every merged field and exits to `skip_exit` with the cursor
    /// untouched. That gives the zero-width path a life outside the search
    /// (it needs no candidate node) and an honest cursor for any follower.
    fn compile_unlabeled_alternatives(
        &mut self,
        alternation: &Pattern,
        alternatives: &[ast::Alternative],
        ctx: PatternCtx,
        skip_exit: SkipExit,
    ) -> Label {
        let PatternCtx {
            exit,
            nav: first_nav,
            capture,
            value: _,
        } = ctx;
        if alternatives.is_empty() {
            return exit;
        }

        // In a suppressed region there is no output shape to keep stable (and a
        // consumed labeled alternation routed here still flows `Value(variant)`, not a struct).
        let alternation_type_id = if self.is_suppressed() {
            None
        } else {
            self.ctx
                .analysis
                .type_analysis
                .expect_pattern_result(alternation)
                .flow
                .type_id()
        };
        let merged_fields =
            alternation_type_id.map(|id| self.ctx.analysis.type_analysis.expect_struct_fields(id));
        let field_completions = alternation_type_id.map(|_| {
            self.ctx
                .analysis
                .type_analysis
                .expect_field_completions(alternation)
        });

        let search_nav = resumable_search_nav(first_nav);
        let alternative_search = AltSearchNav(search_nav);
        let alternative_routing = self.alternative_routing(alternatives, exit);

        let mut successors = Vec::new();
        let mut zero_width = Vec::new();
        for (alternative_idx, alternative) in alternatives.iter().enumerate() {
            let Some(body) = alternative.body() else {
                continue;
            };

            let alternative_exit = alternative_routing.alternative_exit(alternative_idx, exit);

            // Complete every merged field this alternative does not itself
            // produce, so the output shape stays stable. "Produces" means a top-level
            // (bubbling) field — a capture nested in a child scope (`{...} @item`)
            // belongs to that scope, not here. The alternative's inferred bubble is the
            // single source of truth; a syntactic capture walk would miscount nested
            // names and drop a needed completion.
            let completion_effects: Vec<EffectIR> = if let Some(fields) = merged_fields {
                // Only bubbling fields count as provided; a `Value` alternative (a
                // bare reference, suppressed) contributes nothing here.
                let provided: HashSet<Symbol> = match &self
                    .ctx
                    .analysis
                    .type_analysis
                    .expect_pattern_result(&body)
                    .flow
                {
                    PatternFlow::Fields(id) => self
                        .ctx
                        .analysis
                        .type_analysis
                        .expect_struct_fields(*id)
                        .keys()
                        .copied()
                        .collect(),
                    _ => HashSet::new(),
                };
                self.merged_field_completion_effects(
                    field_completions.expect("merged alternation has field completions"),
                    fields,
                    &provided,
                )
            } else {
                vec![]
            };

            let alternative_nav =
                nav_for_alternative(first_nav, alternative_search, &body, &self.anchor_semantics);
            let alternative_span = self.span_id(alternative.syntax(), SpanKind::Alternative);
            let alternative_nullable = self.pattern_is_nullable(&body);
            let alternative_entry = if alternative_nullable {
                let alternative_capture = if let Some(id) = alternative_span {
                    capture
                        .clone()
                        .nest_span(EffectIR::span_start(id.0), EffectIR::span_end(id.0))
                } else {
                    capture.clone()
                };
                // Pruned body: merged effects stay on dominating epsilons —
                // the body's partial-skip paths must not drop them.
                let exit = if alternative_capture.post.is_empty() {
                    alternative_exit
                } else {
                    self.emit_effects_epsilon(
                        alternative_exit,
                        vec![],
                        CaptureEffects::new_post(alternative_capture.post.clone()),
                    )
                };
                let pattern_ctx = PatternCtx {
                    exit,
                    nav: alternative_nav,
                    capture: CaptureEffects::default(),
                    value: false,
                };
                let entry = self.compile_nullable_pattern(&body, pattern_ctx, SkipExit::Fail);
                let mut pre = alternative_capture.pre;
                pre.extend(completion_effects.clone());
                self.wrap_entry_pre(entry, pre)
            } else {
                let alternative_capture = if let Some(id) = alternative_span {
                    capture
                        .clone()
                        .nest_span(EffectIR::span_start(id.0), EffectIR::span_end(id.0))
                        .with_pre_values(completion_effects.clone())
                } else {
                    capture.clone().with_pre_values(completion_effects.clone())
                };
                let pattern_ctx = PatternCtx {
                    exit: alternative_exit,
                    nav: alternative_nav,
                    capture: alternative_capture,
                    value: false,
                };
                self.dispatch_pattern(&body, pattern_ctx)
            };
            successors.push(alternative_entry);

            // Lower the alternative's own zero-width outcome instead of guessing a
            // value from its final field types. The distinction is semantic:
            // an absent field takes its declared completion, while a field that is
            // present through a zero-node value can produce a struct, `""`, or
            // `true`. Reusing the ordinary skippable lowering also preserves
            // capture and alternative spans around that exact outcome.
            if alternative_nullable && let SkipExit::To(skip) = skip_exit {
                let alternative_capture = if let Some(id) = alternative_span {
                    capture
                        .clone()
                        .nest_span(EffectIR::span_start(id.0), EffectIR::span_end(id.0))
                        .with_pre_values(completion_effects)
                } else {
                    capture.clone().with_pre_values(completion_effects)
                };
                let zero_exit = self.emit_effects_epsilon(
                    skip,
                    vec![],
                    CaptureEffects::new_post(alternative_capture.post),
                );
                let pattern_ctx = PatternCtx {
                    exit: zero_exit,
                    nav: alternative_nav,
                    capture: CaptureEffects::default(),
                    value: false,
                };
                let zero_entry = self.compile_zero_width_outcome(&body, pattern_ctx);
                zero_width.push(self.wrap_entry_pre(zero_entry, alternative_capture.pre));
            }
        }

        self.assemble_alternatives(successors, zero_width, search_nav, exit)
    }

    /// Effects that complete every merged field absent from `provided`, resolved
    /// against the enclosing scope.
    fn merged_field_completion_effects(
        &self,
        completions: &FieldCompletions,
        fields: &BTreeMap<Symbol, FieldInfo>,
        provided: &HashSet<Symbol>,
    ) -> Vec<EffectIR> {
        fields
            .iter()
            .filter(|(sym, _)| !provided.contains(*sym))
            .flat_map(|(sym, _)| {
                let completion = completions.completion(*sym);
                let name = self.ctx.analysis.interner.resolve(*sym);
                let member_ref = self
                    .lookup_member_in_scope(name)
                    .expect("alternation field must resolve in enclosing scope");
                let set = EffectIR::with_member(EffectKind::Set, member_ref);
                match completion {
                    FieldCompletion::AlwaysPresent => {
                        unreachable!("an always-present field cannot be absent from an alternative")
                    }
                    FieldCompletion::Absent => vec![EffectIR::null(), set],
                    FieldCompletion::EmptyList => {
                        vec![EffectIR::start_arr(), EffectIR::end_arr(), set]
                    }
                    FieldCompletion::False => {
                        vec![EffectIR::bool_value(false), set]
                    }
                }
            })
            .collect()
    }

    /// A labeled alternation opens each alternative's variant scope
    /// (`VariantOpen`...`VariantClose`) and compiles its payload inside it.
    pub(super) fn compile_labeled_alternation(
        &mut self,
        alternation: &ast::AlternationPattern,
        ctx: PatternCtx,
    ) -> Label {
        let skip_exit = SkipExit::To(ctx.exit);
        self.compile_labeled_alternation_with_exits(alternation, ctx, skip_exit)
    }

    /// [`compile_labeled_alternation`](Self::compile_labeled_alternation) with a distinct zero-width
    /// continuation. A nullable alternative compiles pruned; its zero-width outcome
    /// is lifted past the candidate search — the
    /// variant tags with every payload field at its default (see
    /// [`compile_unlabeled_alternatives`](Self::compile_unlabeled_alternatives)).
    pub(super) fn compile_labeled_alternation_with_exits(
        &mut self,
        alternation: &ast::AlternationPattern,
        ctx: PatternCtx,
        skip_exit: SkipExit,
    ) -> Label {
        let PatternCtx {
            exit,
            nav: first_nav,
            capture,
            value: _,
        } = ctx;
        let alternatives: Vec<_> = alternation.alternatives().collect();
        if alternatives.is_empty() {
            return exit;
        }

        let variant_type_id = self
            .ctx
            .analysis
            .type_analysis
            .expect_pattern_result(&Pattern::Alternation(alternation.clone()))
            .flow
            .type_id()
            .expect("an analyzed labeled alternation must produce a variant type");

        // BTreeMap order gives stable variant indices independent of AST iteration order.
        let TypeShape::Variant(cases) = self
            .ctx
            .analysis
            .type_analysis
            .expect_type_shape(variant_type_id)
        else {
            panic!("an analyzed labeled alternation must produce a variant type");
        };
        let case_info: BTreeMap<Symbol, (u16, TypeId)> = cases
            .iter()
            .enumerate()
            .map(|(idx, (&sym, &type_id))| (sym, (idx as u16, type_id)))
            .collect();

        let search_nav = resumable_search_nav(first_nav);
        let alternative_search = AltSearchNav(search_nav);
        let alternative_routing = self.alternative_routing(&alternatives, exit);

        let mut successors = Vec::new();
        let mut zero_width = Vec::new();
        for (alternative_idx, alternative) in alternatives.iter().enumerate() {
            let Some(body) = alternative.body() else {
                continue;
            };

            let alternative_exit = alternative_routing.alternative_exit(alternative_idx, exit);

            let alternative_nav =
                nav_for_alternative(first_nav, alternative_search, &body, &self.anchor_semantics);

            let label = alternative
                .label()
                .expect("labeled alternative must have label");
            let (case_idx, payload_type_id) = self
                .ctx
                .analysis
                .interner
                .get(label.text())
                .and_then(|sym| case_info.get(&sym))
                .map(|&(idx, type_id)| (idx, type_id))
                .expect("case must exist for labeled alternative");

            let e_effect = EffectIR::with_member(
                EffectKind::VariantOpen,
                MemberRef::new(variant_type_id, case_idx),
            );
            let alternative_span = self.span_id(alternative.syntax(), SpanKind::Alternative);
            if let Some(id) = alternative_span {
                self.bind_span(
                    id,
                    SpanBindingIR::Member(MemberRef::new(variant_type_id, case_idx)),
                );
            }
            let alternative_start = alternative_span.map(|id| EffectIR::span_start(id.0));
            let alternative_end = alternative_span.map(|id| EffectIR::span_end(id.0));

            let alternative_nullable = self.pattern_is_nullable(&body);
            let body_entry = self.with_scope(payload_type_id, |this| {
                if alternative_nullable {
                    let mut close_effects = vec![EffectIR::end_variant()];
                    if let Some(end) = alternative_end.clone() {
                        close_effects.push(end);
                    }
                    let close_exit = this.emit_effects_epsilon(
                        alternative_exit,
                        close_effects,
                        CaptureEffects::new_post(capture.post.clone()),
                    );
                    let pattern_ctx = PatternCtx {
                        exit: close_exit,
                        nav: alternative_nav,
                        capture: CaptureEffects::default(),
                        value: true,
                    };
                    let inner_entry =
                        this.compile_nullable_pattern(&body, pattern_ctx, SkipExit::Fail);
                    let mut entry_pre = capture.pre.clone();
                    if let Some(start) = alternative_start.clone() {
                        entry_pre.push(start);
                    }
                    entry_pre.push(e_effect.clone());
                    this.wrap_entry_pre(inner_entry, entry_pre)
                } else {
                    let alternative_capture = if let (Some(start), Some(end)) =
                        (alternative_start.clone(), alternative_end.clone())
                    {
                        capture
                            .clone()
                            .nest_span(start, end)
                            .nest_scope(e_effect.clone(), EffectIR::end_variant())
                    } else {
                        capture
                            .clone()
                            .nest_scope(e_effect.clone(), EffectIR::end_variant())
                    };
                    let pattern_ctx = PatternCtx {
                        exit: alternative_exit,
                        nav: alternative_nav,
                        capture: alternative_capture,
                        value: false,
                    };
                    this.dispatch_pattern(&body, pattern_ctx)
                }
            });

            successors.push(body_entry);

            if alternative_nullable && let SkipExit::To(skip) = skip_exit {
                let alternative_capture =
                    if let (Some(start), Some(end)) = (alternative_start, alternative_end) {
                        capture
                            .clone()
                            .nest_span(start, end)
                            .nest_scope(e_effect, EffectIR::end_variant())
                    } else {
                        capture
                            .clone()
                            .nest_scope(e_effect, EffectIR::end_variant())
                    };
                let zero_exit = self.emit_effects_epsilon(
                    skip,
                    vec![],
                    CaptureEffects::new_post(alternative_capture.post),
                );
                let zero_entry = self.with_scope(payload_type_id, |this| {
                    let pattern_ctx = PatternCtx {
                        exit: zero_exit,
                        nav: alternative_nav,
                        capture: CaptureEffects::default(),
                        value: true,
                    };
                    this.compile_zero_width_outcome(&body, pattern_ctx)
                });
                zero_width.push(self.wrap_entry_pre(zero_entry, alternative_capture.pre));
            }
        }

        self.assemble_alternatives(successors, zero_width, search_nav, exit)
    }
}
