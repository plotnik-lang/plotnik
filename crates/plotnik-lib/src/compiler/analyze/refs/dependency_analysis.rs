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

    dependencies: DefinitionDependencies,
}

/// Definition-reference graph with the two queries later compiler stages need:
/// inbound-use classification and transitive reachability from a set of roots.
///
/// The graph is indexed by `DefId`, just like `defs`. Keeping this behavior here
/// gives lowering, schema projection, and inspection one authoritative notion of
/// which definition bodies a callable root can demand.
#[derive(Clone, Debug)]
pub(in crate::compiler::analyze::refs) struct DefinitionDependencies {
    outgoing: Vec<Vec<DefId>>,
    has_inbound: Vec<bool>,
}

/// A stable definition subset. Membership is recorded densely; iteration is in
/// `DefId` order regardless of graph traversal order.
#[derive(Clone, Debug)]
pub struct DefinitionReachability {
    reachable: Vec<bool>,
}

impl DefinitionDependencies {
    pub(in crate::compiler::analyze::refs) fn new(outgoing: Vec<Vec<DefId>>) -> Self {
        let mut has_inbound = vec![false; outgoing.len()];
        for dependencies in &outgoing {
            for &target in dependencies {
                let inbound = has_inbound
                    .get_mut(target.index())
                    .expect("definition dependency must target an admitted DefId");
                *inbound = true;
            }
        }
        Self {
            outgoing,
            has_inbound,
        }
    }

    pub(in crate::compiler::analyze::refs) fn has_inbound_references(&self, id: DefId) -> bool {
        *self
            .has_inbound
            .get(id.index())
            .expect("inbound-reference query must use an admitted DefId")
    }

    pub(in crate::compiler::analyze::refs) fn reachable_from(
        &self,
        roots: impl IntoIterator<Item = DefId>,
    ) -> DefinitionReachability {
        let mut reachable = vec![false; self.outgoing.len()];
        let mut pending = roots.into_iter().collect::<Vec<_>>();

        while let Some(def_id) = pending.pop() {
            let admitted = reachable
                .get_mut(def_id.index())
                .expect("reachability root must be an admitted DefId");
            if *admitted {
                continue;
            }
            *admitted = true;
            pending.extend_from_slice(
                self.outgoing
                    .get(def_id.index())
                    .expect("reachable definition must have a dependency row"),
            );
        }

        DefinitionReachability { reachable }
    }
}

impl DefinitionReachability {
    pub fn contains(&self, id: DefId) -> bool {
        *self
            .reachable
            .get(id.index())
            .expect("reachability membership must use an admitted DefId")
    }

    pub fn iter(&self) -> impl Iterator<Item = DefId> + '_ {
        self.reachable
            .iter()
            .enumerate()
            .filter(|(_, reachable)| **reachable)
            .map(|(index, _)| {
                DefId::from_raw(u32::try_from(index).expect("DefId index originated as u32"))
            })
    }
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
        dependencies: DefinitionDependencies,
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
        assert_eq!(
            dependencies.outgoing.len(),
            defs.len(),
            "every definition must have one dependency row",
        );

        Self {
            sccs,
            def_ids_by_sym,
            defs,
            recursive_defs,
            dependencies,
        }
    }

    #[cfg(test)]
    pub(in crate::compiler) fn empty() -> Self {
        Self::new(
            Vec::new(),
            HashMap::new(),
            Vec::new(),
            HashSet::new(),
            DefinitionDependencies::new(Vec::new()),
        )
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

    pub fn has_inbound_references(&self, id: DefId) -> bool {
        self.dependencies.has_inbound_references(id)
    }

    /// Compute the transitive definition set demanded by `roots`.
    ///
    /// The closure includes the roots themselves and is safe for recursive
    /// components. Its iterator is deterministic `DefId` order, making the same
    /// result reusable by output layout and inspection projection.
    pub fn reachable_from(&self, roots: impl IntoIterator<Item = DefId>) -> DefinitionReachability {
        self.dependencies.reachable_from(roots)
    }

    pub fn sccs(&self) -> &[Vec<DefId>] {
        &self.sccs
    }
}
