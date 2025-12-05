use std::sync::Arc;

use tree_sitter::Language;

pub use plotnik_core::{Cardinality, NodeFieldId, NodeTypeId, NodeTypes, StaticNodeTypes};

pub mod builtin;
pub mod dynamic;

pub use builtin::*;

/// User-facing language type. Works with any language (static or dynamic).
pub type Lang = Arc<dyn LangImpl>;

/// Trait providing a unified facade for tree-sitter's Language API
/// combined with our node type constraints.
pub trait LangImpl: Send + Sync {
    fn name(&self) -> &str;

    /// Parse source code into a tree-sitter tree.
    fn parse(&self, source: &str) -> tree_sitter::Tree;

    // ═══════════════════════════════════════════════════════════════════════
    // Resolution                                                [Language API]
    // ═══════════════════════════════════════════════════════════════════════

    fn resolve_node(&self, kind: &str, named: bool) -> Option<NodeTypeId>;
    fn resolve_field(&self, name: &str) -> Option<NodeFieldId>;

    // ═══════════════════════════════════════════════════════════════════════
    // Supertype info                                            [Language API]
    // ═══════════════════════════════════════════════════════════════════════

    fn is_supertype(&self, id: NodeTypeId) -> bool;
    fn subtypes(&self, supertype: NodeTypeId) -> &[u16];

    // ═══════════════════════════════════════════════════════════════════════
    // Root & Extras                                               [node_types]
    // ═══════════════════════════════════════════════════════════════════════

    fn root(&self) -> Option<NodeTypeId>;
    fn is_extra(&self, id: NodeTypeId) -> bool;

    // ═══════════════════════════════════════════════════════════════════════
    // Field constraints                                           [node_types]
    // ═══════════════════════════════════════════════════════════════════════

    fn has_field(&self, node: NodeTypeId, field: NodeFieldId) -> bool;
    fn field_cardinality(&self, node: NodeTypeId, field: NodeFieldId) -> Option<Cardinality>;
    fn valid_field_types(&self, node: NodeTypeId, field: NodeFieldId) -> &[NodeTypeId];
    fn is_valid_field_type(&self, node: NodeTypeId, field: NodeFieldId, child: NodeTypeId) -> bool;

    // ═══════════════════════════════════════════════════════════════════════
    // Children constraints                                        [node_types]
    // ═══════════════════════════════════════════════════════════════════════

    fn children_cardinality(&self, node: NodeTypeId) -> Option<Cardinality>;
    fn valid_child_types(&self, node: NodeTypeId) -> &[NodeTypeId];
    fn is_valid_child_type(&self, node: NodeTypeId, child: NodeTypeId) -> bool;
}

/// Generic language implementation parameterized by node types.
///
/// This struct provides a single implementation of `LangImpl` that works with
/// any `NodeTypes` implementation (static or dynamic).
#[derive(Debug)]
pub struct LangInner<N: NodeTypes> {
    name: String,
    ts_lang: Language,
    node_types: N,
}

impl LangInner<&'static StaticNodeTypes> {
    pub fn new_static(name: &str, ts_lang: Language, node_types: &'static StaticNodeTypes) -> Self {
        Self {
            name: name.to_owned(),
            ts_lang,
            node_types,
        }
    }

    pub fn node_types(&self) -> &'static StaticNodeTypes {
        self.node_types
    }
}

impl<N: NodeTypes + Send + Sync> LangImpl for LangInner<N> {
    fn name(&self) -> &str {
        &self.name
    }

    fn parse(&self, source: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&self.ts_lang)
            .expect("failed to set language");
        parser.parse(source, None).expect("failed to parse source")
    }

    fn resolve_node(&self, kind: &str, named: bool) -> Option<NodeTypeId> {
        let id = self.ts_lang.id_for_node_kind(kind, named);

        // FIX: Disambiguate tree-sitter's ID 0 (could be "end" node or "not found")
        //
        // Tree-sitter's id_for_node_kind has odd semantics:
        // - Returns 0 for "not found"
        // - BUT: ID 0 is also a valid ID for the anonymous "end" sentinel node
        //
        // This creates an ambiguity for anonymous nodes:
        // - id_for_node_kind("end", false) -> 0 (valid)
        // - id_for_node_kind("fake", false) -> 0 (not found)
        //
        // For named nodes, 0 is unambiguous since no named node has ID 0.
        // For anonymous nodes, we must verify via reverse lookup.
        if id == 0 {
            if named {
                return None;
            }
            if self.ts_lang.node_kind_for_id(0) == Some(kind) {
                return Some(0);
            }
            return None;
        }
        Some(id)
    }

    fn resolve_field(&self, name: &str) -> Option<NodeFieldId> {
        self.ts_lang.field_id_for_name(name)
    }

    fn is_supertype(&self, id: NodeTypeId) -> bool {
        self.ts_lang.node_kind_is_supertype(id)
    }

    fn subtypes(&self, supertype: NodeTypeId) -> &[u16] {
        self.ts_lang.subtypes_for_supertype(supertype)
    }

    fn root(&self) -> Option<NodeTypeId> {
        self.node_types.root()
    }

    fn is_extra(&self, id: NodeTypeId) -> bool {
        self.node_types.is_extra(id)
    }

    fn has_field(&self, node: NodeTypeId, field: NodeFieldId) -> bool {
        self.node_types.has_field(node, field)
    }

    fn field_cardinality(&self, node: NodeTypeId, field: NodeFieldId) -> Option<Cardinality> {
        self.node_types.field_cardinality(node, field)
    }

    fn valid_field_types(&self, node: NodeTypeId, field: NodeFieldId) -> &[NodeTypeId] {
        self.node_types.valid_field_types(node, field)
    }

    fn is_valid_field_type(&self, node: NodeTypeId, field: NodeFieldId, child: NodeTypeId) -> bool {
        self.node_types.is_valid_field_type(node, field, child)
    }

    fn children_cardinality(&self, node: NodeTypeId) -> Option<Cardinality> {
        self.node_types.children_cardinality(node)
    }

    fn valid_child_types(&self, node: NodeTypeId) -> &[NodeTypeId] {
        self.node_types.valid_child_types(node)
    }

    fn is_valid_child_type(&self, node: NodeTypeId, child: NodeTypeId) -> bool {
        self.node_types.is_valid_child_type(node, child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "javascript")]
    fn lang_from_name() {
        assert_eq!(from_name("js").unwrap().name(), "javascript");
        assert_eq!(from_name("JavaScript").unwrap().name(), "javascript");
        assert!(from_name("unknown").is_none());
    }

    #[test]
    #[cfg(feature = "go")]
    fn lang_from_name_golang() {
        assert_eq!(from_name("go").unwrap().name(), "go");
        assert_eq!(from_name("golang").unwrap().name(), "go");
        assert_eq!(from_name("GOLANG").unwrap().name(), "go");
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn lang_from_extension() {
        assert_eq!(from_ext("js").unwrap().name(), "javascript");
        assert_eq!(from_ext("mjs").unwrap().name(), "javascript");
    }

    #[test]
    #[cfg(feature = "typescript")]
    fn typescript_and_tsx() {
        assert_eq!(typescript().name(), "typescript");
        assert_eq!(tsx().name(), "tsx");
        assert_eq!(from_ext("ts").unwrap().name(), "typescript");
        assert_eq!(from_ext("tsx").unwrap().name(), "tsx");
    }

    #[test]
    fn all_returns_enabled_langs() {
        let langs = all();
        assert!(!langs.is_empty());
        for lang in &langs {
            assert!(!lang.name().is_empty());
        }
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn resolve_node_and_field() {
        let lang = javascript();

        let func_id = lang.resolve_node("function_declaration", true);
        assert!(func_id.is_some());

        let unknown = lang.resolve_node("nonexistent_node_type", true);
        assert!(unknown.is_none());

        let name_field = lang.resolve_field("name");
        assert!(name_field.is_some());

        let unknown_field = lang.resolve_field("nonexistent_field");
        assert!(unknown_field.is_none());
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn supertype_via_lang_trait() {
        let lang = javascript();

        let expr_id = lang.resolve_node("expression", true).unwrap();
        assert!(lang.is_supertype(expr_id));

        let subtypes = lang.subtypes(expr_id);
        assert!(!subtypes.is_empty());

        let func_id = lang.resolve_node("function_declaration", true).unwrap();
        assert!(!lang.is_supertype(func_id));
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn field_validation_via_trait() {
        let lang = javascript();

        let func_id = lang.resolve_node("function_declaration", true).unwrap();
        let name_field = lang.resolve_field("name").unwrap();
        let body_field = lang.resolve_field("body").unwrap();

        assert!(lang.has_field(func_id, name_field));
        assert!(lang.has_field(func_id, body_field));

        let identifier_id = lang.resolve_node("identifier", true).unwrap();
        assert!(lang.is_valid_field_type(func_id, name_field, identifier_id));

        let statement_block_id = lang.resolve_node("statement_block", true).unwrap();
        assert!(lang.is_valid_field_type(func_id, body_field, statement_block_id));
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn root_via_trait() {
        let lang = javascript();
        let root_id = lang.root();
        assert!(root_id.is_some());

        let program_id = lang.resolve_node("program", true);
        assert_eq!(root_id, program_id);
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn unresolved_returns_none() {
        let lang = javascript();

        assert!(lang.resolve_node("nonexistent_node_type", true).is_none());
        assert!(lang.resolve_field("nonexistent_field").is_none());
    }

    #[test]
    #[cfg(feature = "rust")]
    fn rust_lang_works() {
        let lang = rust();
        let func_id = lang.resolve_node("function_item", true);
        assert!(func_id.is_some());
    }

    /// Demonstrates tree-sitter's odd ID semantics and how our wrapper fixes them.
    ///
    /// Tree-sitter's `id_for_node_kind` returns 0 for both:
    /// 1. The valid "end" sentinel node (anonymous, ID 0)
    /// 2. Any non-existent node
    ///
    /// Our wrapper resolves this correctly.
    #[test]
    #[cfg(feature = "javascript")]
    fn tree_sitter_id_zero_ambiguity() {
        let lang = javascript();

        // For named nodes: 0 unambiguously means "not found"
        assert!(lang.resolve_node("fake_named", true).is_none());

        // For anonymous nodes: we disambiguate via reverse lookup
        let end_resolved = lang.resolve_node("end", false);
        let fake_resolved = lang.resolve_node("totally_fake_node", false);

        assert!(end_resolved.is_some(), "Valid 'end' node should resolve");
        assert_eq!(end_resolved, Some(0), "'end' should have ID 0");

        assert!(
            fake_resolved.is_none(),
            "Non-existent node should be Unresolved"
        );

        // Our wrapper preserves field cleanliness
        assert!(lang.resolve_field("name").is_some());
        assert!(lang.resolve_field("fake_field").is_none());
    }
}
