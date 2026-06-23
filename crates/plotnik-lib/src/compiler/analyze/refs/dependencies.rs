//! Dependency analysis for definitions.
//!
//! Computes the dependency graph of definitions and identifies Strongly Connected
//! Components (SCCs). The computed SCCs are exposed in reverse topological order
//! (leaves first), which is useful for passes that need to process dependencies
//! before dependents (like type inference).

use std::collections::{HashMap, HashSet};

use crate::core::Interner;
use indexmap::{IndexMap, IndexSet};

use crate::compiler::core::DefId;
use crate::compiler::core::SymbolTable;
use crate::compiler::core::{Pattern, Ref};

pub use crate::compiler::core::DependencyAnalysis;

pub fn analyze_dependencies(
    symbol_table: &SymbolTable,
    interner: &mut Interner,
) -> DependencyAnalysis {
    let sccs = SccFinder::find(symbol_table);

    // Tarjan runs `strongconnect` from every symbol-table key, so each def lands in
    // exactly one SCC. Type inference leans on this: a def missing from the partition
    // would never be processed in dependency order, breaking `infer_ref`'s guarantee
    // that a non-recursive target is computed before its referrer.
    debug_assert!(
        {
            let mut seen = HashSet::new();
            sccs.iter().flatten().all(|name| seen.insert(name.as_str()))
                && seen.len() == symbol_table.count()
        },
        "every symbol-table definition must appear in exactly one SCC"
    );

    // Assign DefIds in SCC order (leaves first, so dependencies get lower IDs)
    let mut def_ids_by_sym = HashMap::new();
    let mut def_names = Vec::new();
    let mut recursive_defs = HashSet::new();

    for scc in &sccs {
        let mutually_recursive = scc.len() > 1;
        for name in scc {
            let sym = interner.intern(name);
            let def_id = DefId::from_raw(def_names.len() as u32);
            def_ids_by_sym.insert(sym, def_id);
            def_names.push(sym);

            let self_recursive = symbol_table
                .body(name)
                .is_some_and(|body| super::refs::contains_ref(body, name));
            if mutually_recursive || self_recursive {
                recursive_defs.insert(def_id);
            }
        }
    }

    DependencyAnalysis::new(sccs, def_ids_by_sym, def_names, recursive_defs)
}

struct SccFinder<'a> {
    symbol_table: &'a SymbolTable,
    index: usize,
    stack: Vec<&'a str>,
    on_stack: IndexSet<&'a str>,
    indices: IndexMap<&'a str, usize>,
    lowlinks: IndexMap<&'a str, usize>,
    sccs: Vec<Vec<&'a str>>,
}

impl<'a> SccFinder<'a> {
    fn find(symbol_table: &'a SymbolTable) -> Vec<Vec<String>> {
        let mut finder = Self {
            symbol_table,
            index: 0,
            stack: Vec::new(),
            on_stack: IndexSet::new(),
            indices: IndexMap::new(),
            lowlinks: IndexMap::new(),
            sccs: Vec::new(),
        };

        for name in symbol_table.names() {
            if !finder.indices.contains_key(name as &str) {
                finder.strongconnect(name);
            }
        }

        finder
            .sccs
            .into_iter()
            .map(|scc| scc.into_iter().map(String::from).collect())
            .collect()
    }

    fn strongconnect(&mut self, name: &'a str) {
        self.indices.insert(name, self.index);
        self.lowlinks.insert(name, self.index);
        self.index += 1;
        self.stack.push(name);
        self.on_stack.insert(name);

        if let Some(body) = self.symbol_table.body(name) {
            let refs = collect_refs(body, self.symbol_table);
            for ref_name in refs {
                if !self.indices.contains_key(ref_name) {
                    self.strongconnect(ref_name);
                    let ref_lowlink = self.lowlinks[ref_name];
                    let my_lowlink = self
                        .lowlinks
                        .get_mut(name)
                        .expect("lowlink for name was inserted at the start of strongconnect");
                    *my_lowlink = (*my_lowlink).min(ref_lowlink);
                } else if self.on_stack.contains(ref_name) {
                    let ref_index = self.indices[ref_name];
                    let my_lowlink = self
                        .lowlinks
                        .get_mut(name)
                        .expect("lowlink for name was inserted at the start of strongconnect");
                    *my_lowlink = (*my_lowlink).min(ref_index);
                }
            }
        }

        if self.lowlinks[name] == self.indices[name] {
            let mut scc = Vec::new();
            loop {
                let w = self
                    .stack
                    .pop()
                    .expect("SCC stack holds every on-stack node until its root pops");
                self.on_stack.swap_remove(&w);
                let done = w == name;
                scc.push(w);
                if done {
                    break;
                }
            }
            self.sccs.push(scc);
        }
    }
}

/// Collect references to definitions within the symbol table.
///
/// Returns only refs that point to defined names (filters out node kind references).
pub(super) fn collect_refs<'a>(
    pattern: &Pattern,
    symbol_table: &'a SymbolTable,
) -> IndexSet<&'a str> {
    let mut refs = IndexSet::new();
    for descendant in pattern.syntax().descendants() {
        let Some(r) = Ref::cast(descendant) else {
            continue;
        };
        let Some(name_tok) = r.name() else { continue };
        let Some(key) = symbol_table.defined_name(name_tok.text()) else {
            continue;
        };
        refs.insert(key);
    }
    refs
}
