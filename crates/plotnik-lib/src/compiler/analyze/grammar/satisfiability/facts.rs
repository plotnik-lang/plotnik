use std::collections::HashMap;

use crate::compiler::analyze::grammar::satisfiability::automaton::KindConstraint;
use crate::core::NodeKindId;
use crate::core::grammar::{Grammar, SurfaceRealizer, VarId};

/// The grammar body that realizes a matched node's child structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum NodeRealizer {
    Leaf,
    Var(VarId),
}

impl NodeRealizer {
    fn of_surface(surface: SurfaceRealizer) -> Self {
        Self::of_body(surface.body)
    }

    pub(super) fn of_body(body: Option<VarId>) -> Self {
        body.map(NodeRealizer::Var).unwrap_or(NodeRealizer::Leaf)
    }
}

/// Grammar-wide indexes reused by the primary solve and diagnostic probes. They
/// are independent of anchor mode and query automata, so relaxed probes can share
/// them instead of rebuilding the same maps for every reported culprit.
pub(super) struct GrammarFacts {
    /// Kind -> realizers that may surface that tree kind: the variable named for it,
    /// plus every aliased step occurrence surfacing it.
    realizers_by_kind: HashMap<NodeKindId, Vec<NodeRealizer>>,
    /// Concrete named, non-supertype kinds a wildcard parent could be.
    parent_candidate_kinds: Vec<NodeKindId>,
    /// Visible extra kinds (comments), and the named subset, for extra-consumption.
    extras: Vec<NodeKindId>,
    named_extras: Vec<NodeKindId>,
}

impl GrammarFacts {
    pub(super) fn from_grammar(grammar: &Grammar) -> Self {
        let (extras, named_extras) = extra_kinds(grammar);
        let realizers_by_kind = build_realizers_by_kind(grammar);
        let parent_candidate_kinds = build_parent_candidate_kinds(grammar, &realizers_by_kind);
        Self {
            realizers_by_kind,
            parent_candidate_kinds,
            extras,
            named_extras,
        }
    }

    pub(super) fn realizers_of(&self, kind: NodeKindId) -> &[NodeRealizer] {
        self.realizers_by_kind
            .get(&kind)
            .map(Vec::as_slice)
            .unwrap_or(&[])
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

/// Index every kind to the realizers that can realize it: the variable named for the
/// kind, and every step occurrence that surfaces it (aliases included).
fn build_realizers_by_kind(grammar: &Grammar) -> HashMap<NodeKindId, Vec<NodeRealizer>> {
    grammar
        .structure()
        .surface_realizers_by_kind()
        .into_iter()
        .map(|(kind, surfaces)| {
            let realizers = surfaces.into_iter().map(NodeRealizer::of_surface).collect();
            (kind, realizers)
        })
        .collect()
}

fn build_parent_candidate_kinds(
    grammar: &Grammar,
    realizers_by_kind: &HashMap<NodeKindId, Vec<NodeRealizer>>,
) -> Vec<NodeKindId> {
    let mut candidates: Vec<NodeKindId> = realizers_by_kind
        .keys()
        .copied()
        .filter(|&kind| {
            !grammar.is_anonymous_node(kind)
                && !grammar.is_supertype(kind)
                && !grammar.is_token(kind)
        })
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
