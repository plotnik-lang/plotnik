//! Definition-nullability pre-pass.
//!
//! A definition is *nullable* when its body can match zero nodes (`A = (x)?`,
//! `A = {(a)? (b)?}`, an alias to such a definition, …). A call to a nullable
//! definition may return zero-width, but the caller's return address carries a
//! fixed sibling navigation that assumes the candidate was consumed — the
//! zero-width return would step over an unmatched node. So `compile_ref`
//! inlines nullable bodies at the call site instead, where the ordinary
//! split-exit machinery gives the skip path its own continuation
//! (see `compile_ref_inline` in the lowering).
//!
//! Mirrors the `def_arity` pre-pass: a fixpoint over the definition graph in
//! reverse-topological SCC order, so lowering never guesses.

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

fn pattern_nullable(
    pattern: &Pattern,
    nullable: &HashSet<DefId>,
    deps: &DependencyAnalysis,
    interner: &Interner,
) -> bool {
    match pattern {
        Pattern::NodePattern(_) | Pattern::TokenPattern(_) => false,
        // A nullable value would have arity Many, which field values reject
        // upstream ("field cannot match a sequence") — mirror that verdict.
        Pattern::FieldPattern(_) => false,
        Pattern::QuantifiedPattern(q) => {
            let Some(inner) = q.inner() else {
                // Recovery stub with no inner: void, never admitted for
                // execution — matches `def_arity`'s One recovery.
                return false;
            };
            match q.quantifier_kind() {
                Some(QuantifierKind::Optional | QuantifierKind::ZeroOrMore) => true,
                Some(QuantifierKind::OneOrMore) | None => {
                    pattern_nullable(&inner, nullable, deps, interner)
                }
            }
        }
        Pattern::CapturedPattern(c) => c
            .inner()
            .is_some_and(|inner| pattern_nullable(&inner, nullable, deps, interner)),
        // A sequence matches zero-width only when every item does. An empty
        // sequence compiles to a pass-through, so `all` on nothing is right.
        Pattern::SeqPattern(s) => s
            .children()
            .all(|item| pattern_nullable(&item, nullable, deps, interner)),
        Pattern::Union(u) => {
            u.branches()
                .filter_map(|b| b.body())
                .any(|body| pattern_nullable(&body, nullable, deps, interner))
                || u.patterns()
                    .any(|p| pattern_nullable(&p, nullable, deps, interner))
        }
        Pattern::Enum(e) => e
            .branches()
            .filter_map(|b| b.body())
            .any(|body| pattern_nullable(&body, nullable, deps, interner)),
        Pattern::DefRef(r) => r
            .name()
            .and_then(|n| deps.def_id_for_name(interner, n.text()))
            .is_some_and(|id| nullable.contains(&id)),
    }
}
