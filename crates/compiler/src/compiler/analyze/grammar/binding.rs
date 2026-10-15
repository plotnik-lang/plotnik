use indexmap::IndexMap;

use crate::core::grammar::GrammarIdentity;
use crate::core::{Interner, NodeFieldId, NodeKind, NodeKindId, Symbol};

#[derive(Clone, Debug, Default)]
pub struct GrammarBinding {
    node_kind_ids: IndexMap<NodeKind<Symbol>, NodeKindId>,
    node_field_ids: IndexMap<Symbol, NodeFieldId>,
    identity: Option<GrammarIdentity>,
}

impl GrammarBinding {
    pub fn resolve_named_kind(&self, sym: Symbol) -> Option<NodeKindId> {
        self.node_kind_ids.get(&NodeKind::Named(sym)).copied()
    }

    pub(crate) fn expect_named_kind(&self, sym: Symbol) -> NodeKindId {
        self.resolve_named_kind(sym)
            .expect("grammar-bound named node kind must be present")
    }

    pub fn resolve_anonymous_kind(&self, sym: Symbol) -> Option<NodeKindId> {
        self.node_kind_ids.get(&NodeKind::Anonymous(sym)).copied()
    }

    pub(crate) fn expect_anonymous_kind(&self, sym: Symbol) -> NodeKindId {
        self.resolve_anonymous_kind(sym)
            .expect("grammar-bound anonymous token kind must be present")
    }

    pub fn resolve_field(&self, sym: Symbol) -> Option<NodeFieldId> {
        self.node_field_ids.get(&sym).copied()
    }

    pub(crate) fn expect_field(&self, sym: Symbol) -> NodeFieldId {
        self.resolve_field(sym)
            .expect("grammar-bound field name must be present")
    }

    pub fn kind_name(&self, id: NodeKindId, interner: &Interner) -> Option<String> {
        let sym = self.node_kind_ids.iter().find_map(|(kind, &kind_id)| {
            (kind_id == id).then_some(match kind {
                NodeKind::Named(sym) | NodeKind::Anonymous(sym) => *sym,
            })
        })?;
        interner.try_resolve(sym).map(str::to_string)
    }

    pub fn field_name(&self, id: NodeFieldId, interner: &Interner) -> Option<String> {
        let sym = self
            .node_field_ids
            .iter()
            .find_map(|(&sym, &field_id)| (field_id == id).then_some(sym))?;
        interner.try_resolve(sym).map(str::to_string)
    }

    pub fn kind_entries(
        &self,
    ) -> impl ExactSizeIterator<Item = (NodeKind<Symbol>, NodeKindId)> + '_ {
        self.node_kind_ids.iter().map(|(&kind, &id)| (kind, id))
    }

    pub fn field_entries(&self) -> impl ExactSizeIterator<Item = (Symbol, NodeFieldId)> + '_ {
        self.node_field_ids.iter().map(|(&sym, &id)| (sym, id))
    }

    pub fn identity(&self) -> Option<&GrammarIdentity> {
        self.identity.as_ref()
    }
}

#[derive(Default)]
pub struct GrammarBindingBuilder {
    node_kind_ids: IndexMap<NodeKind<Symbol>, NodeKindId>,
    node_field_ids: IndexMap<Symbol, NodeFieldId>,
    identity: Option<GrammarIdentity>,
}

impl GrammarBindingBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn identity(&mut self, identity: Option<GrammarIdentity>) {
        self.identity = identity;
    }

    pub(crate) fn insert_node_kind_id(&mut self, key: NodeKind<Symbol>, id: NodeKindId) {
        self.node_kind_ids.entry(key).or_insert(id);
    }

    pub(crate) fn insert_node_field_id(&mut self, sym: Symbol, id: NodeFieldId) {
        self.node_field_ids.entry(sym).or_insert(id);
    }

    pub fn finish(self) -> GrammarBinding {
        GrammarBinding {
            node_kind_ids: self.node_kind_ids,
            node_field_ids: self.node_field_ids,
            identity: self.identity,
        }
    }
}
