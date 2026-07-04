//! Definition-arity pre-pass.
//!
//! Arity, unlike output types, needs no SCC deferral: a reference's arity is
//! its target's arity, computable to a fixpoint over the definition graph
//! before inference runs. Inference then never guesses. The old `Arity::One`
//! placeholder for recursive targets tainted every delegating aggregate — a
//! pure-alias body `A = (B)` recorded B's assumed arity as A's own — and let
//! captures on recursive multi-node void definitions slip the
//! single-referent check.

use std::collections::HashMap;

use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::type_shape::Arity;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::Pattern;
use crate::core::Interner;

/// Compute every definition's arity. Mirrors the arity half of inference
/// exactly; `infer_and_register` asserts the two agree.
pub(super) fn compute_def_arities(
    interner: &Interner,
    symbol_table: &SymbolTable,
    dependency_analysis: &DependencyAnalysis,
) -> HashMap<DefId, Arity> {
    let mut arities = HashMap::new();

    // Reverse-topological SCC order: references into earlier SCCs are final.
    // Within an SCC, members start at One (the lattice bottom); `combine` is
    // monotone on the two-point lattice, so iteration converges.
    for scc in dependency_analysis.sccs() {
        for &def_id in scc {
            arities.insert(def_id, Arity::One);
        }
        loop {
            let mut changed = false;
            for &def_id in scc {
                let name = interner.resolve(dependency_analysis.def_name_sym(def_id));
                let body = symbol_table
                    .body(name)
                    .expect("dependency analysis only assigns DefIds to symbol-table definitions");
                let arity = pattern_arity(body, &arities, dependency_analysis, interner);
                if arities.insert(def_id, arity) != Some(arity) {
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    arities
}

fn pattern_arity(
    pattern: &Pattern,
    arities: &HashMap<DefId, Arity>,
    deps: &DependencyAnalysis,
    interner: &Interner,
) -> Arity {
    match pattern {
        Pattern::NodePattern(_) | Pattern::TokenPattern(_) | Pattern::FieldPattern(_) => Arity::One,
        // A quantifier spans a variable range. A recovery stub with no inner
        // is void, matching inference.
        Pattern::QuantifiedPattern(q) => {
            if q.inner().is_some() {
                Arity::Many
            } else {
                Arity::One
            }
        }
        Pattern::CapturedPattern(c) => c.inner().map_or(Arity::One, |inner| {
            pattern_arity(&inner, arities, deps, interner)
        }),
        Pattern::SeqPattern(s) => {
            let mut children = s.children();
            let Some(first) = children.next() else {
                return Arity::One;
            };
            if children.next().is_some() {
                return Arity::Many;
            }
            pattern_arity(&first, arities, deps, interner)
        }
        Pattern::Union(u) => {
            let mut combined = Arity::One;
            for branch in u.branches() {
                if let Some(body) = branch.body() {
                    combined = combined.combine(pattern_arity(&body, arities, deps, interner));
                }
            }
            for p in u.patterns() {
                combined = combined.combine(pattern_arity(&p, arities, deps, interner));
            }
            combined
        }
        Pattern::Enum(e) => {
            let mut combined = Arity::One;
            for branch in e.branches() {
                if let Some(body) = branch.body() {
                    combined = combined.combine(pattern_arity(&body, arities, deps, interner));
                }
            }
            combined
        }
        // An undefined reference answers One, matching inference's void
        // recovery (the dangling name is already diagnosed upstream).
        Pattern::DefRef(r) => r
            .name()
            .and_then(|n| deps.def_id_for_name(interner, n.text()))
            .and_then(|id| arities.get(&id).copied())
            .unwrap_or(Arity::One),
    }
}
