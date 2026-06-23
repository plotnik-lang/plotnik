//! The dependency-analysis result: SCC partition and `DefId` assignments over the
//! definition graph, admitted past the dependency-analysis boundary.

use std::collections::{HashMap, HashSet};

use crate::{DefId, Interner, Symbol};

#[derive(Clone, Debug, Default)]
pub struct DependencyAnalysis {
    /// Strongly connected components in reverse topological order.
    ///
    /// - `sccs[0]` has no dependencies (or depends only on things not in this list).
    /// - `sccs.last()` depends on everything else.
    /// - Definitions within an SCC are mutually recursive.
    /// - Every definition in the symbol table appears exactly once.
    sccs: Vec<Vec<String>>,

    def_ids_by_sym: HashMap<Symbol, DefId>,

    def_names: Vec<Symbol>,

    recursive_defs: HashSet<DefId>,
}

impl DependencyAnalysis {
    pub fn new(
        sccs: Vec<Vec<String>>,
        def_ids_by_sym: HashMap<Symbol, DefId>,
        def_names: Vec<Symbol>,
        recursive_defs: HashSet<DefId>,
    ) -> Self {
        Self {
            sccs,
            def_ids_by_sym,
            def_names,
            recursive_defs,
        }
    }

    pub fn def_id_for_sym(&self, sym: Symbol) -> Option<DefId> {
        self.def_ids_by_sym.get(&sym).copied()
    }

    pub fn def_id_for_name(&self, interner: &Interner, name: &str) -> Option<DefId> {
        let sym = interner.get(name)?;
        self.def_id_for_sym(sym)
    }

    pub fn def_name_sym(&self, id: DefId) -> Symbol {
        self.def_names[id.index()]
    }

    pub fn def_name<'a>(&self, interner: &'a Interner, id: DefId) -> &'a str {
        interner.resolve(self.def_names[id.index()])
    }

    /// Number of definitions.
    pub fn def_count(&self) -> usize {
        self.def_names.len()
    }

    /// True if the definition is in a mutual recursion group (SCC > 1) or references itself.
    pub fn is_recursive_def(&self, id: DefId) -> bool {
        self.recursive_defs.contains(&id)
    }

    pub fn sccs(&self) -> &[Vec<String>] {
        &self.sccs
    }
}
