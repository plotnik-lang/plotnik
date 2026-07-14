//! Grammar binding: the bind pass's resolution table and its builder.
//!
//! `GrammarBinding` is the immutable table; `GrammarBindingBuilder` is the
//! accumulator the bind pass fills. The data and its builder live together; the
//! bind pass that drives the builder lives in `bind`.

use indexmap::IndexMap;

use crate::core::grammar::GrammarIdentity;
use crate::core::{Interner, NodeFieldId, NodeKind, NodeKindId, Symbol};

/// Resolution table produced by the bind pass: the query's node-kind and field
/// symbols bound to the selected grammar's ids, in both directions.
///
/// Immutable once binding produces it; build one with `GrammarBindingBuilder`.
#[derive(Clone, Debug, Default)]
pub struct GrammarBinding {
    node_kind_ids: IndexMap<NodeKind<Symbol>, NodeKindId>,
    node_field_ids: IndexMap<Symbol, NodeFieldId>,
    identity: Option<GrammarIdentity>,
}

impl GrammarBinding {
    /// Freeze finished resolution tables into the binding. The bind pass's builder
    /// is the only constructor; callers that already have admitted compiler state
    /// should use the expecting accessors below.
    fn new(
        node_kind_ids: IndexMap<NodeKind<Symbol>, NodeKindId>,
        node_field_ids: IndexMap<Symbol, NodeFieldId>,
    ) -> Self {
        Self {
            node_kind_ids,
            node_field_ids,
            identity: None,
        }
    }

    /// Grammar id bound to a named node kind, or `None` when the query never
    /// names it (an unconstrained match).
    pub fn resolve_named_kind(&self, sym: Symbol) -> Option<NodeKindId> {
        self.node_kind_ids.get(&NodeKind::Named(sym)).copied()
    }

    /// Grammar id for a named kind referenced by an admitted query.
    ///
    /// Missing here means analysis/bind and lower disagree about trusted state; widening to a
    /// wildcard would compile the wrong query.
    pub(crate) fn expect_named_kind(&self, sym: Symbol) -> NodeKindId {
        self.resolve_named_kind(sym)
            .expect("grammar-bound named node kind must be present")
    }

    /// Grammar id bound to an anonymous (literal-token) node kind.
    pub fn resolve_anonymous_kind(&self, sym: Symbol) -> Option<NodeKindId> {
        self.node_kind_ids.get(&NodeKind::Anonymous(sym)).copied()
    }

    /// Grammar id for a literal token referenced by an admitted query.
    pub(crate) fn expect_anonymous_kind(&self, sym: Symbol) -> NodeKindId {
        self.resolve_anonymous_kind(sym)
            .expect("grammar-bound anonymous token kind must be present")
    }

    /// Grammar id bound to a field name.
    pub fn resolve_field(&self, sym: Symbol) -> Option<NodeFieldId> {
        self.node_field_ids.get(&sym).copied()
    }

    /// Grammar id for a field referenced by an admitted query.
    pub(crate) fn expect_field(&self, sym: Symbol) -> NodeFieldId {
        self.resolve_field(sym)
            .expect("grammar-bound field name must be present")
    }

    /// Name of a bound node-kind id — reverse lookup for trace/debug rendering.
    /// O(n) scan; intended for diagnostics, not hot paths.
    pub fn kind_name(&self, id: NodeKindId, interner: &Interner) -> Option<String> {
        let sym = self.node_kind_ids.iter().find_map(|(kind, &kind_id)| {
            (kind_id == id).then_some(match kind {
                NodeKind::Named(sym) | NodeKind::Anonymous(sym) => *sym,
            })
        })?;
        interner.try_resolve(sym).map(str::to_string)
    }

    /// Name of a bound field id — reverse lookup for trace/debug rendering.
    pub fn field_name(&self, id: NodeFieldId, interner: &Interner) -> Option<String> {
        let sym = self
            .node_field_ids
            .iter()
            .find_map(|(&sym, &field_id)| (field_id == id).then_some(sym))?;
        interner.try_resolve(sym).map(str::to_string)
    }

    /// Every node-kind binding, in resolution order — the emit node-kind table.
    pub fn kind_entries(
        &self,
    ) -> impl ExactSizeIterator<Item = (NodeKind<Symbol>, NodeKindId)> + '_ {
        self.node_kind_ids.iter().map(|(&kind, &id)| (kind, id))
    }

    /// Every field binding, in resolution order — the emit field table.
    pub fn field_entries(&self) -> impl ExactSizeIterator<Item = (Symbol, NodeFieldId)> + '_ {
        self.node_field_ids.iter().map(|(&sym, &id)| (sym, id))
    }

    pub fn identity(&self) -> Option<&GrammarIdentity> {
        self.identity.as_ref()
    }
}

/// Mutable accumulator for a [`GrammarBinding`], owned by the bind pass.
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

    /// Record the first NodeKindId seen for a node kind, keeping the existing entry.
    pub(crate) fn insert_node_kind_id(&mut self, key: NodeKind<Symbol>, id: NodeKindId) {
        self.node_kind_ids.entry(key).or_insert(id);
    }

    /// Record the first NodeFieldId seen for a field, keeping the existing entry.
    pub(crate) fn insert_node_field_id(&mut self, sym: Symbol, id: NodeFieldId) {
        self.node_field_ids.entry(sym).or_insert(id);
    }

    /// Freeze the accumulated resolution tables into an immutable [`GrammarBinding`].
    pub fn finish(self) -> GrammarBinding {
        let mut binding = GrammarBinding::new(self.node_kind_ids, self.node_field_ids);
        binding.identity = self.identity;
        binding
    }
}
