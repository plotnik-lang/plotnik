//! Definition-nullability pre-pass.
//!
//! A definition is *nullable* when its body can match zero nodes (`A = (x)?`,
//! `A = {(a)? (b)?}`, an alias to such a definition, …). A call to a nullable
//! definition may return empty, but the caller's return address carries a
//! fixed sibling navigation that assumes the candidate was consumed — the
//! empty return would step over an unmatched node. So `compile_ref`
//! inlines nullable bodies at the call site instead, where the ordinary
//! split-exit machinery gives the skip path its own continuation
//! (see `compile_ref_inline` in the lowering).
//!
//! Mirrors the definition root-extent pre-pass: a fixpoint over the definition
//! graph in reverse-topological SCC order, so lowering never guesses.

use std::collections::HashSet;

use super::names::SymbolTable;
use super::refs::DependencyAnalysis;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::{Pattern, QuantifierKind};
use crate::core::Interner;

pub(crate) fn compute_nullable_defs(
    interner: &Interner,
    symbol_table: &SymbolTable,
    dependency_analysis: &DependencyAnalysis,
) -> HashSet<DefId> {
    let mut nullable = HashSet::new();

    // Members start non-nullable (the lattice bottom); insertion is monotone,
    // so iteration converges. Recursion rules reject non-consuming cycles, so
    // in practice in-SCC nullability settles in one pass, but the loop keeps
    // the pre-pass correct by construction rather than by that argument.
    for scc in dependency_analysis.sccs() {
        loop {
            let mut changed = false;
            for &def_id in scc {
                if nullable.contains(&def_id) {
                    continue;
                }
                let name = interner.resolve(dependency_analysis.def_name_sym(def_id));
                let body = symbol_table
                    .body(name)
                    .expect("dependency analysis only assigns DefIds to symbol-table definitions");
                if pattern_nullable(body, &nullable, dependency_analysis, interner) {
                    nullable.insert(def_id);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    nullable
}

/// Whether a pattern can match zero nodes, given the set of nullable
/// definitions (for `DefRef` leaves). Shared by the fixpoint above and by
/// lowering, which prunes empty paths in quantifier iterations and
/// alternation branches.
pub(crate) fn pattern_nullable(
    pattern: &Pattern,
    nullable: &HashSet<DefId>,
    deps: &DependencyAnalysis,
    interner: &Interner,
) -> bool {
    match pattern {
        Pattern::NodePattern(_) | Pattern::TokenPattern(_) => false,
        // A nullable value has `RootExtent::Other`, which field values reject
        // upstream ("field cannot match a sequence") — mirror that verdict.
        Pattern::FieldPattern(_) => false,
        Pattern::QuantifiedPattern(q) => {
            let Some(inner) = q.inner() else {
                // Recovery stub with no inner: no-value, never admitted for
                // execution — matches the root-extent pass's `SingleNode`
                // recovery.
                return false;
            };
            match q.quantifier_kind() {
                Some(QuantifierKind::Optional | QuantifierKind::ZeroOrMore) => true,
                // A `+` always consumes: lowering prunes the empty path
                // of a nullable element inside quantifier iterations, so even
                // `+` over a nullable inner cannot match zero nodes.
                Some(QuantifierKind::OneOrMore) => false,
                None => pattern_nullable(&inner, nullable, deps, interner),
            }
        }
        Pattern::CapturedPattern(c) => c
            .inner()
            .is_some_and(|inner| pattern_nullable(&inner, nullable, deps, interner)),
        // A sequence matches empty only when every item does. An empty
        // sequence compiles to a pass-through, so `all` on nothing is right.
        Pattern::SeqPattern(s) => s
            .children()
            .all(|item| pattern_nullable(&item, nullable, deps, interner)),
        Pattern::Alternation(alternation) => {
            alternation
                .alternatives()
                .filter_map(|alternative| alternative.body())
                .any(|body| pattern_nullable(&body, nullable, deps, interner))
                || alternation
                    .patterns()
                    .any(|p| pattern_nullable(&p, nullable, deps, interner))
        }
        Pattern::DefRef(r) => r
            .name()
            .and_then(|n| deps.def_id_for_name(interner, n.text()))
            .is_some_and(|id| nullable.contains(&id)),
    }
}
