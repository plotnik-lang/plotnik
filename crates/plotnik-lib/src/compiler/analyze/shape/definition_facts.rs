//! Grammar-independent structural facts for definition bodies.
//!
//! These facts are shared by type inference, entry-point selection, and
//! lowering. Their definition-level answers are retained once per analyzed
//! query so later phases never recompute the fixed points.

use crate::compiler::analyze::refs::DefinitionGraph;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::{Pattern, QuantifierKind};
use crate::core::Interner;

use super::RootExtent;
use super::anchor_context::AnchorContextAnalysis;

#[derive(Clone, Copy, Debug)]
struct DefinitionFact {
    nullable: bool,
    root_extent: RootExtent,
    requires_external_anchor_context: bool,
}

/// Frozen structural classification of every admitted definition.
#[derive(Clone, Debug)]
pub(crate) struct DefinitionFacts {
    facts: Vec<DefinitionFact>,
}

impl DefinitionFacts {
    pub(crate) fn analyze(
        interner: &Interner,
        definitions: &DefinitionGraph,
        anchor_contexts: &AnchorContextAnalysis<'_>,
    ) -> Self {
        let mut facts = definitions
            .ids_in_def_id_order()
            .map(|def_id| DefinitionFact {
                nullable: false,
                root_extent: RootExtent::SingleNode,
                requires_external_anchor_context: anchor_contexts
                    .definition_requires_external_context(def_id),
            })
            .collect::<Vec<_>>();

        compute_nullability(&mut facts, interner, definitions);
        compute_root_extents(&mut facts, interner, definitions);

        Self { facts }
    }

    pub(crate) fn is_nullable(&self, def_id: DefId) -> bool {
        self.fact(def_id).nullable
    }

    pub(crate) fn root_extent(&self, def_id: DefId) -> RootExtent {
        self.fact(def_id).root_extent
    }

    pub(crate) fn is_entry_point_eligible(&self, def_id: DefId) -> bool {
        let fact = self.fact(def_id);
        fact.root_extent == RootExtent::SingleNode && !fact.requires_external_anchor_context
    }

    pub(crate) fn pattern_is_nullable(
        &self,
        pattern: &Pattern,
        definitions: &DefinitionGraph,
        interner: &Interner,
    ) -> bool {
        pattern_nullable(pattern, &self.facts, definitions, interner)
    }

    fn fact(&self, def_id: DefId) -> &DefinitionFact {
        fact(&self.facts, def_id)
    }
}

fn fact(facts: &[DefinitionFact], def_id: DefId) -> &DefinitionFact {
    facts
        .get(def_id.index())
        .expect("structural-fact lookup must use an admitted DefId")
}

fn compute_nullability(
    facts: &mut [DefinitionFact],
    interner: &Interner,
    definitions: &DefinitionGraph,
) {
    // `false` is the lattice bottom and insertion is monotone. Recursion
    // validation rejects non-consuming cycles, but the fixed point keeps this
    // analysis correct independently of that later admission rule.
    for scc in definitions.sccs() {
        loop {
            let mut changed = false;
            for &def_id in scc {
                if fact(facts, def_id).nullable {
                    continue;
                }
                let body = definitions.definition(def_id).body();
                if pattern_nullable(body, facts, definitions, interner) {
                    facts[def_id.index()].nullable = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
}

/// Whether a pattern can match zero nodes.
///
/// Repetition deliberately admits only consuming iterations, so `+` over a
/// nullable body is not itself nullable.
fn pattern_nullable(
    pattern: &Pattern,
    facts: &[DefinitionFact],
    definitions: &DefinitionGraph,
    interner: &Interner,
) -> bool {
    match pattern {
        Pattern::NamedNodePattern(_)
        | Pattern::AnonymousNodePattern(_)
        | Pattern::NodeWildcard(_) => false,
        // A nullable value has `RootExtent::NotSingleNode`, which field values
        // reject upstream ("field cannot match a sequence").
        Pattern::FieldPattern(_) => false,
        Pattern::QuantifiedPattern(quantified) => {
            let Some(inner) = quantified.inner() else {
                // Recovery stubs are never admitted for execution.
                return false;
            };
            match quantified.quantifier_kind() {
                Some(QuantifierKind::Optional | QuantifierKind::ZeroOrMore) => true,
                Some(QuantifierKind::OneOrMore) => false,
                None => pattern_nullable(&inner, facts, definitions, interner),
            }
        }
        Pattern::CapturedPattern(capture) => capture
            .inner()
            .is_some_and(|inner| pattern_nullable(&inner, facts, definitions, interner)),
        Pattern::SeqPattern(sequence) => sequence
            .children()
            .all(|item| pattern_nullable(&item, facts, definitions, interner)),
        Pattern::Alternation(alternation) => {
            alternation.alternatives().any(|alternative| {
                alternative
                    .body()
                    .is_some_and(|body| pattern_nullable(&body, facts, definitions, interner))
            }) || alternation
                .patterns()
                .any(|pattern| pattern_nullable(&pattern, facts, definitions, interner))
        }
        Pattern::DefRef(reference) => reference
            .name()
            .and_then(|name| definitions.id_for_name(interner, name.text()))
            .is_some_and(|def_id| fact(facts, def_id).nullable),
    }
}

fn compute_root_extents(
    facts: &mut [DefinitionFact],
    interner: &Interner,
    definitions: &DefinitionGraph,
) {
    // SCC order is leaves first. `SingleNode` is the optimistic lattice bottom;
    // `combine` can only widen it to `NotSingleNode`.
    for scc in definitions.sccs() {
        loop {
            let mut changed = false;
            for &def_id in scc {
                let body = definitions.definition(def_id).body();
                let extent = pattern_root_extent(body, facts, definitions, interner);
                let current = &mut facts[def_id.index()].root_extent;
                if *current != extent {
                    *current = extent;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
}

fn pattern_root_extent(
    pattern: &Pattern,
    facts: &[DefinitionFact],
    definitions: &DefinitionGraph,
    interner: &Interner,
) -> RootExtent {
    match pattern {
        Pattern::NamedNodePattern(_)
        | Pattern::AnonymousNodePattern(_)
        | Pattern::NodeWildcard(_)
        | Pattern::FieldPattern(_) => RootExtent::SingleNode,
        Pattern::QuantifiedPattern(quantified) => {
            if quantified.inner().is_some() {
                RootExtent::NotSingleNode
            } else {
                RootExtent::SingleNode
            }
        }
        Pattern::CapturedPattern(capture) => {
            capture.inner().map_or(RootExtent::SingleNode, |inner| {
                pattern_root_extent(&inner, facts, definitions, interner)
            })
        }
        Pattern::SeqPattern(sequence) => {
            let mut children = sequence.children();
            let Some(first) = children.next() else {
                return RootExtent::SingleNode;
            };
            if children.next().is_some() {
                return RootExtent::NotSingleNode;
            }
            pattern_root_extent(&first, facts, definitions, interner)
        }
        Pattern::Alternation(alternation) => {
            let mut combined = RootExtent::SingleNode;
            for alternative in alternation.alternatives() {
                if let Some(body) = alternative.body() {
                    combined =
                        combined.combine(pattern_root_extent(&body, facts, definitions, interner));
                }
            }
            for pattern in alternation.patterns() {
                combined =
                    combined.combine(pattern_root_extent(&pattern, facts, definitions, interner));
            }
            combined
        }
        // Name resolution owns undefined-reference diagnostics. The recovery
        // shape stays single-node to avoid cascading errors.
        Pattern::DefRef(reference) => reference
            .name()
            .and_then(|name| definitions.id_for_name(interner, name.text()))
            .map(|def_id| fact(facts, def_id).root_extent)
            .unwrap_or(RootExtent::SingleNode),
    }
}
