//! Dependency analysis for definitions.
//!
//! Computes the dependency graph of definitions and identifies Strongly Connected
//! Components (SCCs). The computed SCCs are exposed in reverse topological order
//! (leaves first), which is useful for passes that need to process dependencies
//! before dependents (like type inference).

use std::collections::{HashMap, HashSet};

use crate::core::Interner;
use indexmap::{IndexMap, IndexSet};

use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::diagnostics::Error;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::{DefRef, Pattern};

use super::dependency_analysis::DefInfo;
pub use super::dependency_analysis::DependencyAnalysis;

pub fn analyze_dependencies(
    symbol_table: &SymbolTable,
    interner: &mut Interner,
    max_depth: u32,
) -> Result<DependencyAnalysis, Error> {
    // `strongconnect` recurses one frame per edge it follows, so a reference chain
    // longer than `max_depth` (`A = (B)`, `B = (C)`, …, thousands deep) overflows the
    // native stack here — a vector the parser's nesting cap never sees, since each such
    // definition is flat. Reject it with the same recursion-limit error the parser
    // raises for deep nesting. Self/mutual recursion stays shallow: a back-edge to an
    // already-visited node takes the non-recursive branch, so only an acyclic chain
    // grows the stack.
    let Some(sccs) = TarjanScc::find(symbol_table, max_depth) else {
        return Err(Error::RecursionLimitExceeded);
    };

    // Tarjan runs `strongconnect` from every symbol-table key, so each def lands in
    // exactly one SCC. Type inference leans on this: a def missing from the partition
    // would never be processed in dependency order, breaking `infer_ref`'s guarantee
    // that a non-recursive target is computed before its referrer.
    assert!(
        {
            let mut seen = HashSet::new();
            sccs.iter().flatten().all(|name| seen.insert(name.as_str()))
                && seen.len() == symbol_table.count()
        },
        "every symbol-table definition must appear in exactly one SCC"
    );

    // Assign DefIds in SCC order (leaves first, so dependencies get lower IDs)
    let mut def_ids_by_sym = HashMap::new();
    let mut defs = Vec::new();
    let mut recursive_defs = HashSet::new();
    let mut scc_ids_by_def = Vec::with_capacity(sccs.len());

    for scc in &sccs {
        let mutually_recursive = scc.len() > 1;
        let mut scc_ids = Vec::with_capacity(scc.len());
        for name in scc {
            let sym = interner.intern(name);
            let source = symbol_table
                .source_id(name)
                .expect("Tarjan SCC member must exist in the symbol table");
            let def_id = DefId::from_raw(defs.len() as u32);
            def_ids_by_sym.insert(sym, def_id);
            defs.push(DefInfo { name: sym, source });
            scc_ids.push(def_id);

            let self_recursive = symbol_table
                .body(name)
                .is_some_and(|body| super::collect::contains_ref(body, name));
            if mutually_recursive || self_recursive {
                recursive_defs.insert(def_id);
            }
        }
        scc_ids_by_def.push(scc_ids);
    }

    Ok(DependencyAnalysis::new(
        scc_ids_by_def,
        def_ids_by_sym,
        defs,
        recursive_defs,
    ))
}

struct TarjanScc<'a> {
    symbol_table: &'a SymbolTable,
    index: usize,
    stack: Vec<&'a str>,
    on_stack: IndexSet<&'a str>,
    indices: IndexMap<&'a str, usize>,
    lowlinks: IndexMap<&'a str, usize>,
    sccs: Vec<Vec<&'a str>>,
    /// Recursion ceiling and current depth: an acyclic reference chain deeper than
    /// this would overflow the native stack, so we stop and flag it instead.
    max_depth: u32,
    depth: u32,
    depth_exceeded: bool,
}

impl<'a> TarjanScc<'a> {
    /// Returns the SCCs, or `None` if an acyclic reference chain ran past `max_depth`
    /// (the caller rejects the query rather than risk a stack overflow).
    fn find(symbol_table: &'a SymbolTable, max_depth: u32) -> Option<Vec<Vec<String>>> {
        let mut finder = Self {
            symbol_table,
            index: 0,
            stack: Vec::new(),
            on_stack: IndexSet::new(),
            indices: IndexMap::new(),
            lowlinks: IndexMap::new(),
            sccs: Vec::new(),
            max_depth,
            depth: 0,
            depth_exceeded: false,
        };

        for name in symbol_table.names() {
            if !finder.indices.contains_key(name as &str) {
                finder.strongconnect(name);
                if finder.depth_exceeded {
                    return None;
                }
            }
        }

        Some(
            finder
                .sccs
                .into_iter()
                .map(|scc| scc.into_iter().map(String::from).collect())
                .collect(),
        )
    }

    fn strongconnect(&mut self, name: &'a str) {
        // One frame per edge followed; stop before the chain outruns the stack. The
        // partial state left behind is discarded — `find` returns `None` and the query
        // is rejected — so we need not unwind it cleanly, only avoid recursing further.
        if self.depth >= self.max_depth {
            self.depth_exceeded = true;
            return;
        }
        self.depth += 1;

        self.indices.insert(name, self.index);
        self.lowlinks.insert(name, self.index);
        self.index += 1;
        self.stack.push(name);
        self.on_stack.insert(name);

        if let Some(body) = self.symbol_table.body(name) {
            let refs = collect_defined_refs(body, self.symbol_table);
            for ref_name in refs {
                if !self.indices.contains_key(ref_name) {
                    self.strongconnect(ref_name);
                    if self.depth_exceeded {
                        self.depth -= 1;
                        return;
                    }
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

        self.depth -= 1;
    }
}

/// Collect references to definitions within the symbol table.
///
/// Returns only refs that point to defined names (filters out node kind references).
pub(super) fn collect_defined_refs<'a>(
    pattern: &Pattern,
    symbol_table: &'a SymbolTable,
) -> IndexSet<&'a str> {
    let mut refs = IndexSet::new();
    for descendant in pattern.syntax().descendants() {
        let Some(r) = DefRef::cast(descendant) else {
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
