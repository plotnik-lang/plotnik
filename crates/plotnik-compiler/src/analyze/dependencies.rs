//! Dependency analysis for definitions.
//!
//! Computes the dependency graph of definitions and identifies Strongly Connected
//! Components (SCCs). The computed SCCs are exposed in reverse topological order
//! (leaves first), which is useful for passes that need to process dependencies
//! before dependents (like type inference).

use std::collections::{HashMap, HashSet};

use indexmap::{IndexMap, IndexSet};
use plotnik_core::{Interner, Symbol};

use super::symbol_table::SymbolTable;
use super::type_check::DefId;
use crate::parser::{Expr, Ref};

/// Result of dependency analysis.
#[derive(Clone, Debug, Default)]
pub struct DependencyAnalysis {
    /// Strongly connected components in reverse topological order.
    ///
    /// - `sccs[0]` has no dependencies (or depends only on things not in this list).
    /// - `sccs.last()` depends on everything else.
    /// - Definitions within an SCC are mutually recursive.
    /// - Every definition in the symbol table appears exactly once.
    pub sccs: Vec<Vec<String>>,

    /// Maps definition name (Symbol) to its DefId.
    name_to_def: HashMap<Symbol, DefId>,

    /// Maps DefId to definition name Symbol (indexed by DefId).
    def_names: Vec<Symbol>,

    /// Set of recursive definition names.
    ///
    /// A definition is recursive if it's in an SCC with >1 member,
    /// or it's a single-member SCC that references itself.
    recursive_defs: HashSet<String>,
}

impl DependencyAnalysis {
    /// Get the DefId for a definition by Symbol.
    pub fn def_id_by_symbol(&self, sym: Symbol) -> Option<DefId> {
        self.name_to_def.get(&sym).copied()
    }

    /// Get the DefId for a definition name (requires interner for lookup).
    pub fn def_id(&self, interner: &Interner, name: &str) -> Option<DefId> {
        // Linear scan - only used during analysis, not hot path
        for (&sym, &def_id) in &self.name_to_def {
            if interner.resolve(sym) == name {
                return Some(def_id);
            }
        }
        None
    }

    /// Get the name Symbol for a DefId.
    pub fn def_name_sym(&self, id: DefId) -> Symbol {
        self.def_names[id.index()]
    }

    /// Get the name string for a DefId.
    pub fn def_name<'a>(&self, interner: &'a Interner, id: DefId) -> &'a str {
        interner.resolve(self.def_names[id.index()])
    }

    /// Number of definitions.
    pub fn def_count(&self) -> usize {
        self.def_names.len()
    }

    /// Get the def_names slice (for seeding TypeContext).
    pub fn def_names(&self) -> &[Symbol] {
        &self.def_names
    }

    /// Get the name_to_def map (for seeding TypeContext).
    pub fn name_to_def(&self) -> &HashMap<Symbol, DefId> {
        &self.name_to_def
    }

    /// Returns true if this definition is recursive.
    ///
    /// A definition is recursive if it's part of a mutual recursion group (SCC > 1),
    /// or it's a single definition that references itself.
    pub fn is_recursive(&self, name: &str) -> bool {
        self.recursive_defs.contains(name)
    }
}

/// Analyze dependencies between definitions.
///
/// Returns the SCCs in reverse topological order, with DefId mappings.
/// The interner is used to intern definition names as Symbols.
pub fn analyze_dependencies(
    symbol_table: &SymbolTable,
    interner: &mut Interner,
) -> DependencyAnalysis {
    let sccs = SccFinder::find(symbol_table);

    // Assign DefIds in SCC order (leaves first, so dependencies get lower IDs)
    let mut name_to_def = HashMap::new();
    let mut def_names = Vec::new();
    let mut recursive_defs = HashSet::new();

    for scc in &sccs {
        // Mark recursive definitions
        if scc.len() > 1 {
            // Mutual recursion: all members are recursive
            recursive_defs.extend(scc.iter().cloned());
        } else if let Some(name) = scc.first()
            && let Some(body) = symbol_table.get(name)
            && super::refs::contains_ref(body, name)
        {
            recursive_defs.insert(name.clone());
        }

        for name in scc {
            let sym = interner.intern(name);
            let def_id = DefId::from_raw(def_names.len() as u32);
            name_to_def.insert(sym, def_id);
            def_names.push(sym);
        }
    }

    DependencyAnalysis {
        sccs,
        name_to_def,
        def_names,
        recursive_defs,
    }
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

        for name in symbol_table.keys() {
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

        if let Some(body) = self.symbol_table.get(name) {
            let refs = collect_refs(body, self.symbol_table);
            for ref_name in refs {
                if !self.indices.contains_key(ref_name) {
                    self.strongconnect(ref_name);
                    let ref_lowlink = self.lowlinks[ref_name];
                    let my_lowlink = self.lowlinks.get_mut(name).unwrap();
                    *my_lowlink = (*my_lowlink).min(ref_lowlink);
                } else if self.on_stack.contains(ref_name) {
                    let ref_index = self.indices[ref_name];
                    let my_lowlink = self.lowlinks.get_mut(name).unwrap();
                    *my_lowlink = (*my_lowlink).min(ref_index);
                }
            }
        }

        if self.lowlinks[name] == self.indices[name] {
            let mut scc = Vec::new();
            loop {
                let w = self.stack.pop().unwrap();
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
/// Returns only refs that point to defined names (filters out node type references).
pub(super) fn collect_refs<'a>(expr: &Expr, symbol_table: &'a SymbolTable) -> IndexSet<&'a str> {
    let mut refs = IndexSet::new();
    for descendant in expr.as_cst().descendants() {
        let Some(r) = Ref::cast(descendant) else {
            continue;
        };
        let Some(name_tok) = r.name() else { continue };
        let Some(key) = symbol_table.keys().find(|&k| k == name_tok.text()) else {
            continue;
        };
        refs.insert(key);
    }
    refs
}
