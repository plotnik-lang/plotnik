use crate::compiler::analyze::grammar::model::KindConstraint;
use crate::core::NodeKindId;
use crate::core::grammar::Grammar;

pub(super) struct GrammarFacts {
    parent_candidate_kinds: Vec<NodeKindId>,
    extras: Vec<NodeKindId>,
    named_extras: Vec<NodeKindId>,
}

impl GrammarFacts {
    pub(super) fn from_grammar(grammar: &Grammar) -> Self {
        let (extras, named_extras) = extra_kinds(grammar);
        let parent_candidate_kinds = build_parent_candidate_kinds(grammar);
        Self {
            parent_candidate_kinds,
            extras,
            named_extras,
        }
    }

    pub(super) fn parent_candidate_kinds(&self) -> &[NodeKindId] {
        &self.parent_candidate_kinds
    }

    pub(super) fn admits_any_extra(&self, grammar: &Grammar, constraint: KindConstraint) -> bool {
        self.extra_candidates(grammar, constraint).next().is_some()
    }

    pub(super) fn extra_candidates(
        &self,
        grammar: &Grammar,
        constraint: KindConstraint,
    ) -> impl Iterator<Item = NodeKindId> + '_ {
        let (single, kinds): (_, &[NodeKindId]) = match constraint {
            KindConstraint::Exact(id) => (grammar.is_extra(id).then_some(id), &[]),
            KindConstraint::AnyNamed => (None, &self.named_extras),
            KindConstraint::AnyNode | KindConstraint::Unconstrained => (None, &self.extras),
        };
        single.into_iter().chain(kinds.iter().copied())
    }
}

fn build_parent_candidate_kinds(grammar: &Grammar) -> Vec<NodeKindId> {
    let mut candidates: Vec<NodeKindId> = grammar
        .structure()
        .realizers()
        .keys()
        .copied()
        .filter(|&kind| !grammar.is_anonymous_node(kind) && !grammar.is_supertype(kind))
        .collect();
    candidates.sort_unstable();
    candidates
}

fn extra_kinds(grammar: &Grammar) -> (Vec<NodeKindId>, Vec<NodeKindId>) {
    let extras = grammar.extra_node_kinds().to_vec();
    let named = extras
        .iter()
        .copied()
        .filter(|&kind| !grammar.is_anonymous_node(kind))
        .collect();
    (extras, named)
}
