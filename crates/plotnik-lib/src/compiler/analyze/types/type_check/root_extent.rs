//! Definition root-extent pre-pass.
//!
//! Root extent, unlike result types, needs no SCC deferral: a reference has its
//! target's extent, computable to a fixpoint over the definition graph before
//! inference runs. Inference then never guesses. The former optimistic
//! `SingleNode` placeholder for recursive targets tainted every delegating
//! aggregate and let captures on recursive multi-node match-only definitions
//! slip the single-referent check.

use std::collections::HashMap;

use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::RootExtent;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::Pattern;
use crate::core::Interner;

/// Compute every definition's root extent. Inference independently derives the
/// same extent and asserts that the two views agree.
pub(super) fn compute_definition_root_extents(
    interner: &Interner,
    symbol_table: &SymbolTable,
    dependency_analysis: &DependencyAnalysis,
) -> HashMap<DefId, RootExtent> {
    let mut extents = HashMap::new();

    // Reverse-topological SCC order: references into earlier SCCs are final.
    // Within an SCC, members start at `SingleNode`, the optimistic lattice
    // bottom. `combine` is monotone, so iteration converges.
    for scc in dependency_analysis.sccs() {
        for &def_id in scc {
            extents.insert(def_id, RootExtent::SingleNode);
        }
        loop {
            let mut changed = false;
            for &def_id in scc {
                let name = interner.resolve(dependency_analysis.def_name_sym(def_id));
                let body = symbol_table
                    .body(name)
                    .expect("dependency analysis only assigns DefIds to symbol-table definitions");
                let extent = pattern_root_extent(body, &extents, dependency_analysis, interner);
                if extents.insert(def_id, extent) != Some(extent) {
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    extents
}

fn pattern_root_extent(
    pattern: &Pattern,
    extents: &HashMap<DefId, RootExtent>,
    deps: &DependencyAnalysis,
    interner: &Interner,
) -> RootExtent {
    match pattern {
        Pattern::NamedNodePattern(_)
        | Pattern::AnonymousNodePattern(_)
        | Pattern::NodeWildcard(_)
        | Pattern::FieldPattern(_) => RootExtent::SingleNode,
        // A quantifier has variable top-level extent. A recovery stub with no
        // inner defaults to `SingleNode` to avoid cascading diagnostics; it is
        // never an admitted definition.
        Pattern::QuantifiedPattern(q) => {
            if q.inner().is_some() {
                RootExtent::Other
            } else {
                RootExtent::SingleNode
            }
        }
        Pattern::CapturedPattern(c) => c.inner().map_or(RootExtent::SingleNode, |inner| {
            pattern_root_extent(&inner, extents, deps, interner)
        }),
        Pattern::SeqPattern(s) => {
            let mut children = s.children();
            let Some(first) = children.next() else {
                return RootExtent::SingleNode;
            };
            if children.next().is_some() {
                return RootExtent::Other;
            }
            pattern_root_extent(&first, extents, deps, interner)
        }
        Pattern::Alternation(alternation) => {
            let mut combined = RootExtent::SingleNode;
            for alternative in alternation.alternatives() {
                if let Some(body) = alternative.body() {
                    combined =
                        combined.combine(pattern_root_extent(&body, extents, deps, interner));
                }
            }
            for p in alternation.patterns() {
                combined = combined.combine(pattern_root_extent(&p, extents, deps, interner));
            }
            combined
        }
        // An undefined reference defaults to `SingleNode`, matching inference's
        // no-value recovery; the dangling name is diagnosed upstream.
        Pattern::DefRef(r) => r
            .name()
            .and_then(|n| deps.def_id_for_name(interner, n.text()))
            .and_then(|id| extents.get(&id).copied())
            .unwrap_or(RootExtent::SingleNode),
    }
}
