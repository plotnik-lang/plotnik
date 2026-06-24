//! Grammar binding: the link pass's resolution table and its builder.
//!
//! `GrammarBinding` is the immutable table; `GrammarBindingBuilder` is the
//! accumulator the link pass fills. The data and its builder live together; the
//! link pass that drives the builder lives in `link`.

use indexmap::IndexMap;

use crate::core::{Interner, NodeFieldId, NodeKind, NodeKindId, Symbol};

/// Resolution table produced by the link pass: the query's node-kind and field
/// symbols bound to the target grammar's ids, in both directions.
///
/// Immutable once linking produces it; build one with `GrammarBindingBuilder`.
#[derive(Clone, Debug, Default)]
pub struct GrammarBinding {
    node_kind_ids: IndexMap<NodeKind<Symbol>, NodeKindId>,
    node_field_ids: IndexMap<Symbol, NodeFieldId>,
}

impl GrammarBinding {
    /// Freeze finished resolution tables into the binding. The link pass's builder
    /// is the intended caller.
    pub fn new(
        node_kind_ids: IndexMap<NodeKind<Symbol>, NodeKindId>,
        node_field_ids: IndexMap<Symbol, NodeFieldId>,
    ) -> Self {
        Self {
            node_kind_ids,
            node_field_ids,
        }
    }

    /// Grammar id bound to a named node kind, or `None` when the query never
    /// names it (an unconstrained match).
    pub fn resolve_named_kind(&self, sym: Symbol) -> Option<NodeKindId> {
        self.node_kind_ids.get(&NodeKind::Named(sym)).copied()
    }

    /// Grammar id bound to an anonymous (literal-token) node kind.
    pub fn resolve_anonymous_kind(&self, sym: Symbol) -> Option<NodeKindId> {
        self.node_kind_ids.get(&NodeKind::Anonymous(sym)).copied()
    }

    /// Grammar id bound to a field name.
    pub fn resolve_field(&self, sym: Symbol) -> Option<NodeFieldId> {
        self.node_field_ids.get(&sym).copied()
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
}

/// Mutable accumulator for a [`GrammarBinding`], owned by the link pass.
#[derive(Default)]
pub struct GrammarBindingBuilder {
    node_kind_ids: IndexMap<NodeKind<Symbol>, NodeKindId>,
    node_field_ids: IndexMap<Symbol, NodeFieldId>,
}

impl GrammarBindingBuilder {
    pub fn new() -> Self {
        Self::default()
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
        GrammarBinding::new(self.node_kind_ids, self.node_field_ids)
    }
}
