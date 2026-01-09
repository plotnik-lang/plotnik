use std::num::NonZeroU16;
use std::sync::Arc;

use arborium_tree_sitter::Language;

use plotnik_core::grammar::Grammar;
use plotnik_core::{Cardinality, DynamicNodeTypes, NodeFieldId, NodeTypeId, NodeTypes, RawNode};

pub mod builtin;
pub mod dynamic;

#[cfg(test)]
mod lib_tests;

pub use builtin::*;

/// User-facing language type. Works with any language (static or dynamic).
pub type Lang = Arc<dyn LangImpl>;

/// Trait providing a unified facade for tree-sitter's Language API
/// combined with our node type constraints.
pub trait LangImpl: Send + Sync {
    fn name(&self) -> &str;

    /// Parse source code into a tree-sitter tree.
    fn parse(&self, source: &str) -> arborium_tree_sitter::Tree;

    fn resolve_named_node(&self, kind: &str) -> Option<NodeTypeId>;
    fn resolve_anonymous_node(&self, kind: &str) -> Option<NodeTypeId>;
    fn resolve_field(&self, name: &str) -> Option<NodeFieldId>;

    // Enumeration methods for suggestions
    fn all_named_node_kinds(&self) -> Vec<&'static str>;
    fn all_field_names(&self) -> Vec<&'static str>;
    fn node_type_name(&self, node_type_id: NodeTypeId) -> Option<&'static str>;
    fn field_name(&self, field_id: NodeFieldId) -> Option<&'static str>;
    fn fields_for_node_type(&self, node_type_id: NodeTypeId) -> Vec<&'static str>;

    fn is_supertype(&self, node_type_id: NodeTypeId) -> bool;
    fn subtypes(&self, supertype: NodeTypeId) -> &[u16];

    fn root(&self) -> Option<NodeTypeId>;
    fn is_extra(&self, node_type_id: NodeTypeId) -> bool;

    fn has_field(&self, node_type_id: NodeTypeId, node_field_id: NodeFieldId) -> bool;
    fn field_cardinality(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
    ) -> Option<Cardinality>;
    fn valid_field_types(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
    ) -> &[NodeTypeId];
    fn is_valid_field_type(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
        child: NodeTypeId,
    ) -> bool;

    fn children_cardinality(&self, node_type_id: NodeTypeId) -> Option<Cardinality>;
    fn valid_child_types(&self, node_type_id: NodeTypeId) -> &[NodeTypeId];
    fn is_valid_child_type(&self, node_type_id: NodeTypeId, child: NodeTypeId) -> bool;

    /// Get the grammar for this language (for grammar-verify).
    fn grammar(&self) -> &Grammar;
}

/// Language implementation with embedded grammar and node types.
#[derive(Debug)]
pub struct LangInner {
    name: String,
    ts_lang: Language,
    node_types: DynamicNodeTypes,
    grammar: Grammar,
}

impl LangInner {
    /// Create a new language from raw node types and grammar.
    pub fn new(name: &str, ts_lang: Language, raw_nodes: Vec<RawNode>, grammar: Grammar) -> Self {
        let node_types = DynamicNodeTypes::build(
            &raw_nodes,
            |kind, named| {
                let id = ts_lang.id_for_node_kind(kind, named);
                NonZeroU16::new(id)
            },
            |field_name| ts_lang.field_id_for_name(field_name),
        );

        Self {
            name: name.to_owned(),
            ts_lang,
            node_types,
            grammar,
        }
    }

    pub fn node_types(&self) -> &DynamicNodeTypes {
        &self.node_types
    }
}

impl LangImpl for LangInner {
    fn name(&self) -> &str {
        &self.name
    }

    fn parse(&self, source: &str) -> arborium_tree_sitter::Tree {
        let mut parser = arborium_tree_sitter::Parser::new();
        parser
            .set_language(&self.ts_lang)
            .expect("failed to set language");
        parser.parse(source, None).expect("failed to parse source")
    }

    fn resolve_named_node(&self, kind: &str) -> Option<NodeTypeId> {
        let id = self.ts_lang.id_for_node_kind(kind, true);
        NonZeroU16::new(id)
    }

    fn resolve_anonymous_node(&self, kind: &str) -> Option<NodeTypeId> {
        let id = self.ts_lang.id_for_node_kind(kind, false);
        // Node ID 0 is tree-sitter internal; we never obtain it via cursor walk.
        NonZeroU16::new(id)
    }

    fn resolve_field(&self, name: &str) -> Option<NodeFieldId> {
        self.ts_lang.field_id_for_name(name)
    }

    fn all_named_node_kinds(&self) -> Vec<&'static str> {
        let count = self.ts_lang.node_kind_count();
        (0..count as u16)
            .filter(|&id| self.ts_lang.node_kind_is_named(id))
            .filter_map(|id| self.ts_lang.node_kind_for_id(id))
            .collect()
    }

    fn all_field_names(&self) -> Vec<&'static str> {
        let count = self.ts_lang.field_count();
        (1..=count as u16)
            .filter_map(|id| self.ts_lang.field_name_for_id(id))
            .collect()
    }

    fn node_type_name(&self, node_type_id: NodeTypeId) -> Option<&'static str> {
        self.ts_lang.node_kind_for_id(node_type_id.get())
    }

    fn field_name(&self, field_id: NodeFieldId) -> Option<&'static str> {
        self.ts_lang.field_name_for_id(field_id.get())
    }

    fn fields_for_node_type(&self, node_type_id: NodeTypeId) -> Vec<&'static str> {
        let count = self.ts_lang.field_count();
        (1..=count as u16)
            .filter_map(|id| {
                let field_id = std::num::NonZeroU16::new(id)?;
                if self.node_types.has_field(node_type_id, field_id) {
                    self.ts_lang.field_name_for_id(id)
                } else {
                    None
                }
            })
            .collect()
    }

    fn is_supertype(&self, node_type_id: NodeTypeId) -> bool {
        self.ts_lang.node_kind_is_supertype(node_type_id.get())
    }

    fn subtypes(&self, supertype: NodeTypeId) -> &[u16] {
        self.ts_lang.subtypes_for_supertype(supertype.get())
    }

    fn root(&self) -> Option<NodeTypeId> {
        self.node_types.root()
    }

    fn is_extra(&self, node_type_id: NodeTypeId) -> bool {
        self.node_types.is_extra(node_type_id)
    }

    fn has_field(&self, node_type_id: NodeTypeId, node_field_id: NodeFieldId) -> bool {
        self.node_types.has_field(node_type_id, node_field_id)
    }

    fn field_cardinality(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
    ) -> Option<Cardinality> {
        self.node_types
            .field_cardinality(node_type_id, node_field_id)
    }

    fn valid_field_types(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
    ) -> &[NodeTypeId] {
        self.node_types
            .valid_field_types(node_type_id, node_field_id)
    }

    fn is_valid_field_type(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
        child: NodeTypeId,
    ) -> bool {
        self.node_types
            .is_valid_field_type(node_type_id, node_field_id, child)
    }

    fn children_cardinality(&self, node_type_id: NodeTypeId) -> Option<Cardinality> {
        self.node_types.children_cardinality(node_type_id)
    }

    fn valid_child_types(&self, node_type_id: NodeTypeId) -> &[NodeTypeId] {
        self.node_types.valid_child_types(node_type_id)
    }

    fn is_valid_child_type(&self, node_type_id: NodeTypeId, child: NodeTypeId) -> bool {
        self.node_types.is_valid_child_type(node_type_id, child)
    }

    fn grammar(&self) -> &Grammar {
        &self.grammar
    }
}
