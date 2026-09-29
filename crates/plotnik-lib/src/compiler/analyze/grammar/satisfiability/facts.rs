use crate::compiler::analyze::grammar::model::KindConstraint;
use crate::core::NodeKindId;
use crate::core::grammar::Grammar;

/// Grammar-wide indexes reused by the primary solve and diagnostic probes. They
/// are independent of anchor mode and query automata, so relaxed probes can share
/// them instead of rebuilding the same maps for every reported culprit.
pub(super) struct GrammarFacts {
    /// Concrete named, non-supertype kinds a wildcard parent could be.
    parent_candidate_kinds: Vec<NodeKindId>,
    /// Visible extra kinds (comments), and the named subset, for extra-consumption.
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
        match constraint {
            KindConstraint::Exact(id) => grammar.is_extra(id),
            KindConstraint::AnyNamed => !self.named_extras.is_empty(),
            KindConstraint::AnyNode | KindConstraint::Unconstrained => !self.extras.is_empty(),
        }
    }

    pub(super) fn any_extra_admitted_by(
        &self,
        grammar: &Grammar,
        constraint: KindConstraint,
        mut predicate: impl FnMut(NodeKindId) -> bool,
    ) -> bool {
        match constraint {
            KindConstraint::Exact(id) => grammar.is_extra(id) && predicate(id),
            KindConstraint::AnyNamed => self.named_extras.iter().copied().any(predicate),
            KindConstraint::AnyNode | KindConstraint::Unconstrained => {
                self.extras.iter().copied().any(predicate)
            }
        }
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
    // Extras are mostly lexical tokens (comments), so they live in the grammar's
    // extra set, not the syntax-variable structure.
    let extras = grammar.extra_node_kinds().to_vec();
    let named = extras
        .iter()
        .copied()
        .filter(|&kind| !grammar.is_anonymous_node(kind))
        .collect();
    (extras, named)
}
