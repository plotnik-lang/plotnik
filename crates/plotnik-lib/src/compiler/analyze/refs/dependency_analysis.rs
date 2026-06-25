//! The dependency-analysis result: SCC partition and `DefId` assignments over the
//! definition graph, admitted past the dependency-analysis boundary.

use std::collections::{HashMap, HashSet};

use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::ids::DefId;
use crate::core::{Interner, Symbol};

#[derive(Clone, Debug)]
pub struct DependencyAnalysis {
    /// Strongly connected components in reverse topological order.
    ///
    /// - `sccs[0]` has no dependencies (or depends only on things not in this list).
    /// - `sccs.last()` depends on everything else.
    /// - Definitions within an SCC are mutually recursive.
    /// - Every definition in the symbol table appears exactly once.
    sccs: Vec<Vec<DefId>>,

    def_ids_by_sym: HashMap<Symbol, DefId>,

    defs: Vec<DefInfo>,

    recursive_defs: HashSet<DefId>,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::compiler::analyze::refs) struct DefInfo {
    pub(in crate::compiler::analyze::refs) name: Symbol,
    pub(in crate::compiler::analyze::refs) source: SourceId,
}

impl DependencyAnalysis {
    pub(in crate::compiler::analyze::refs) fn new(
        sccs: Vec<Vec<DefId>>,
        def_ids_by_sym: HashMap<Symbol, DefId>,
        defs: Vec<DefInfo>,
        recursive_defs: HashSet<DefId>,
    ) -> Self {
        assert_eq!(
            sccs.iter().flatten().count(),
            defs.len(),
            "every SCC member must correspond to exactly one definition",
        );
        assert_eq!(
            def_ids_by_sym.len(),
            defs.len(),
            "every definition name must have exactly one DefId",
        );

        for (index, def) in defs.iter().copied().enumerate() {
            let def_id = def_ids_by_sym
                .get(&def.name)
                .copied()
                .expect("every definition record must be indexed by symbol");
            assert_eq!(
                def_id.index(),
                index,
                "DefId index must point at its definition name",
            );
        }

        for scc in &sccs {
            for def_id in scc {
                assert!(def_id.index() < defs.len(), "SCC DefId must be within defs",);
            }
        }

        for (sym, def_id) in &def_ids_by_sym {
            let def_name = defs
                .get(def_id.index())
                .map(|def| def.name)
                .expect("DefId index must be within defs");
            assert_eq!(
                def_name, *sym,
                "DefId reverse lookup must point back to its symbol",
            );
        }

        for def_id in &recursive_defs {
            assert!(
                def_id.index() < defs.len(),
                "recursive DefId must be within defs",
            );
        }

        Self {
            sccs,
            def_ids_by_sym,
            defs,
            recursive_defs,
        }
    }

    #[cfg(test)]
    pub(in crate::compiler) fn empty() -> Self {
        Self::new(Vec::new(), HashMap::new(), Vec::new(), HashSet::new())
    }

    pub fn def_id_for_sym(&self, sym: Symbol) -> Option<DefId> {
        self.def_ids_by_sym.get(&sym).copied()
    }

    pub fn def_id_for_name(&self, interner: &Interner, name: &str) -> Option<DefId> {
        let sym = interner.get(name)?;
        self.def_id_for_sym(sym)
    }

    pub fn def_name_sym(&self, id: DefId) -> Symbol {
        self.defs[id.index()].name
    }

    pub fn def_source_id(&self, id: DefId) -> SourceId {
        self.defs[id.index()].source
    }

    /// True if the definition is in a mutual recursion group (SCC > 1) or references itself.
    pub fn is_recursive_def(&self, id: DefId) -> bool {
        self.recursive_defs.contains(&id)
    }

    pub fn sccs(&self) -> &[Vec<DefId>] {
        &self.sccs
    }
}
