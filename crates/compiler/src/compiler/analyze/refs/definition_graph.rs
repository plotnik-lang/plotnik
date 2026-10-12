//! Canonical definition records and their reference graph.
//!
//! Dependency analysis consumes the transient string-keyed name-resolution map
//! into this `DefId`-indexed representation. Every later compiler phase reads
//! names, sources, bodies, SCCs, and graph relationships from this one owner.

use std::collections::HashMap;

use crate::compiler::analyze::Located;
use crate::compiler::diagnostics::{SourceId, Span};
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::{self, DefRef, Pattern};
use crate::core::{Interner, Symbol};

pub(crate) struct Definition {
    name: Symbol,
    source: SourceId,
    body: Pattern,
    outgoing: Vec<DefId>,
    recursive: bool,
    has_inbound: bool,
}

impl Definition {
    pub(super) fn new(name: Symbol, source: SourceId, body: Pattern, outgoing: Vec<DefId>) -> Self {
        Self {
            name,
            source,
            body,
            outgoing,
            recursive: false,
            has_inbound: false,
        }
    }

    pub(crate) fn name(&self) -> Symbol {
        self.name
    }

    pub(crate) fn source(&self) -> SourceId {
        self.source
    }

    pub(crate) fn body(&self) -> &Pattern {
        &self.body
    }

    pub(crate) fn located_body(&self) -> Located<Pattern> {
        Located::new(self.source, self.body.clone())
    }

    pub(crate) fn span(&self) -> Span {
        let definition = self
            .body
            .syntax()
            .parent()
            .and_then(ast::Def::cast)
            .expect("admitted definition body belongs to a definition");
        Span::new(self.source, definition.syntax().text_range())
    }
}

pub(crate) struct DefinitionGraph {
    /// Strongly connected components in reverse topological order.
    ///
    /// `sccs[0]` has no dependencies, definitions within an SCC are mutually
    /// recursive, and every admitted definition appears exactly once.
    sccs: Vec<Vec<DefId>>,
    ids_by_symbol: HashMap<Symbol, DefId>,
    definitions: Vec<Definition>,
    /// Definition declaration order across source-map order.
    declaration_order: Vec<DefId>,
    /// The one resolved target for each authored reference occurrence.
    target_by_reference: HashMap<DefRef, DefId>,
}

/// A stable definition subset. Membership is recorded densely; iteration is in
/// `DefId` order regardless of graph traversal order.
#[derive(Clone)]
pub(crate) struct DefinitionReachability {
    reachable: Vec<bool>,
}

impl DefinitionGraph {
    pub(in crate::compiler::analyze::refs) fn new(
        sccs: Vec<Vec<DefId>>,
        ids_by_symbol: HashMap<Symbol, DefId>,
        mut definitions: Vec<Definition>,
        declaration_order: Vec<DefId>,
        target_by_reference: HashMap<DefRef, DefId>,
    ) -> Self {
        let mut has_inbound = vec![false; definitions.len()];
        for definition in &definitions {
            for &target in &definition.outgoing {
                let inbound = has_inbound
                    .get_mut(target.index())
                    .expect("definition dependency must target an admitted DefId");
                *inbound = true;
            }
        }
        for (definition, has_inbound) in definitions.iter_mut().zip(has_inbound) {
            definition.has_inbound = has_inbound;
        }

        for scc in &sccs {
            let mutually_recursive = scc.len() > 1;
            for &def_id in scc {
                let definition = definitions
                    .get_mut(def_id.index())
                    .expect("SCC DefId must address an admitted definition");
                definition.recursive = mutually_recursive || definition.outgoing.contains(&def_id);
            }
        }

        let graph = Self {
            sccs,
            ids_by_symbol,
            definitions,
            declaration_order,
            target_by_reference,
        };
        graph.assert_well_formed();
        graph
    }

    fn assert_well_formed(&self) {
        let definition_count = self.definitions.len();
        assert_eq!(
            self.ids_by_symbol.len(),
            definition_count,
            "every definition name must have exactly one DefId",
        );
        assert_eq!(
            self.sccs.iter().flatten().count(),
            definition_count,
            "every definition must appear in exactly one SCC",
        );
        assert_eq!(
            self.declaration_order.len(),
            definition_count,
            "declaration order must contain every definition exactly once",
        );
        let mut in_scc = vec![false; definition_count];
        for &def_id in self.sccs.iter().flatten() {
            let seen = in_scc
                .get_mut(def_id.index())
                .expect("SCC DefId must be within definitions");
            assert!(!*seen, "a definition must not appear in multiple SCCs");
            *seen = true;
        }

        let mut in_declaration_order = vec![false; definition_count];
        for &def_id in &self.declaration_order {
            let seen = in_declaration_order
                .get_mut(def_id.index())
                .expect("declaration-order DefId must be within definitions");
            assert!(
                !*seen,
                "a definition must not appear twice in declaration order"
            );
            *seen = true;
        }

        for (index, definition) in self.definitions.iter().enumerate() {
            let def_id = self
                .ids_by_symbol
                .get(&definition.name)
                .copied()
                .expect("every definition record must be indexed by name");
            assert_eq!(
                def_id.index(),
                index,
                "DefId index must point at its definition record",
            );
        }
        assert!(
            self.target_by_reference
                .values()
                .all(|target| target.index() < definition_count),
            "every reference target must be an admitted definition"
        );
    }

    pub(crate) fn len(&self) -> usize {
        self.definitions.len()
    }

    pub(crate) fn ids_in_def_id_order(&self) -> impl Iterator<Item = DefId> + '_ {
        (0..self.len()).map(|index| {
            DefId::from_raw(u32::try_from(index).expect("definition count originated as u32"))
        })
    }

    pub(crate) fn ids_in_declaration_order(&self) -> &[DefId] {
        &self.declaration_order
    }

    pub(crate) fn definition(&self, id: DefId) -> &Definition {
        self.definitions
            .get(id.index())
            .expect("definition lookup must use an admitted DefId")
    }

    pub(crate) fn id_for_symbol(&self, name: Symbol) -> Option<DefId> {
        self.ids_by_symbol.get(&name).copied()
    }

    pub(crate) fn id_for_name(&self, interner: &Interner, name: &str) -> Option<DefId> {
        self.id_for_symbol(interner.get(name)?)
    }

    pub(crate) fn reference_target(&self, reference: &DefRef) -> Option<DefId> {
        self.target_by_reference.get(reference).copied()
    }

    pub(crate) fn expect_reference_target(&self, reference: &DefRef) -> DefId {
        self.reference_target(reference)
            .expect("analyzed reference must resolve to a definition")
    }

    pub(crate) fn is_recursive(&self, id: DefId) -> bool {
        self.definition(id).recursive
    }

    pub(crate) fn has_inbound_references(&self, id: DefId) -> bool {
        self.definition(id).has_inbound
    }

    /// Compute the transitive definition set demanded by `roots`.
    ///
    /// The closure includes the roots themselves and is safe for recursive
    /// components. Its iterator is deterministic `DefId` order.
    pub(crate) fn reachable_from(
        &self,
        roots: impl IntoIterator<Item = DefId>,
    ) -> DefinitionReachability {
        let mut reachable = vec![false; self.len()];
        let mut pending = roots.into_iter().collect::<Vec<_>>();

        while let Some(def_id) = pending.pop() {
            let admitted = reachable
                .get_mut(def_id.index())
                .expect("reachability root must be an admitted DefId");
            if *admitted {
                continue;
            }
            *admitted = true;
            pending.extend_from_slice(&self.definition(def_id).outgoing);
        }

        DefinitionReachability { reachable }
    }

    pub(crate) fn sccs(&self) -> &[Vec<DefId>] {
        &self.sccs
    }
}

impl DefinitionReachability {
    pub(crate) fn contains(&self, id: DefId) -> bool {
        *self
            .reachable
            .get(id.index())
            .expect("reachability membership must use an admitted DefId")
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = DefId> + '_ {
        self.reachable
            .iter()
            .enumerate()
            .filter(|(_, reachable)| **reachable)
            .map(|(index, _)| {
                DefId::from_raw(u32::try_from(index).expect("DefId index originated as u32"))
            })
    }
}
