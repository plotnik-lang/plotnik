//! The dependency-analysis result: SCC partition and `DefId` assignments over the
//! definition graph, admitted past the dependency-analysis boundary.

use std::collections::{HashMap, HashSet};

use crate::compiler::core::{DefId, Interner, Symbol};

#[derive(Clone, Debug)]
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
    pub(in crate::compiler) fn new(
        sccs: Vec<Vec<String>>,
        def_ids_by_sym: HashMap<Symbol, DefId>,
        def_names: Vec<Symbol>,
        recursive_defs: HashSet<DefId>,
    ) -> Self {
        assert_eq!(
            sccs.iter().flatten().count(),
            def_names.len(),
            "every SCC member must correspond to exactly one definition",
        );
        assert_eq!(
            def_ids_by_sym.len(),
            def_names.len(),
            "every definition name must have exactly one DefId",
        );

        for (index, sym) in def_names.iter().copied().enumerate() {
            let def_id = def_ids_by_sym
                .get(&sym)
                .copied()
                .expect("every def_names entry must be indexed by symbol");
            assert_eq!(
                def_id.index(),
                index,
                "DefId index must point at its definition name",
            );
        }

        for (sym, def_id) in &def_ids_by_sym {
            let def_name = def_names
                .get(def_id.index())
                .copied()
                .expect("DefId index must be within def_names");
            assert_eq!(
                def_name, *sym,
                "DefId reverse lookup must point back to its symbol",
            );
        }

        for def_id in &recursive_defs {
            assert!(
                def_id.index() < def_names.len(),
                "recursive DefId must be within def_names",
            );
        }

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

    /// True if the definition is in a mutual recursion group (SCC > 1) or references itself.
    pub fn is_recursive_def(&self, id: DefId) -> bool {
        self.recursive_defs.contains(&id)
    }

    pub fn sccs(&self) -> &[Vec<String>] {
        &self.sccs
    }
}
