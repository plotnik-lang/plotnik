use std::sync::Arc;

use tree_sitter::Language;

use plotnik_core::{Cardinality, NodeFieldId, NodeTypeId, NodeTypes, StaticNodeTypes};

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

    fn resolve_named_node(&self, kind: &str) -> Option<NodeTypeId> {
        let id = self.ts_lang.id_for_node_kind(kind, true);
        // For named nodes, 0 always means "not found"
        (id != 0).then_some(id)
    }

    fn resolve_anonymous_node(&self, kind: &str) -> Option<NodeTypeId> {
        let id = self.ts_lang.id_for_node_kind(kind, false);
        // Tree-sitter returns 0 for both "not found" AND the valid anonymous "end" node.
        // We disambiguate via reverse lookup.
        if id != 0 {
            return Some(id);
        }
        (self.ts_lang.node_kind_for_id(0) == Some(kind)).then_some(0)
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
        self.ts_lang.node_kind_for_id(node_type_id)
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
        self.ts_lang.node_kind_is_supertype(node_type_id)
    }

    fn subtypes(&self, supertype: NodeTypeId) -> &[u16] {
        self.ts_lang.subtypes_for_supertype(supertype)
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

        let func_id = lang.resolve_named_node("function_declaration");
        assert!(func_id.is_some());

        let unknown = lang.resolve_named_node("nonexistent_node_type");
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

        let expr_id = lang.resolve_named_node("expression").unwrap();
        assert!(lang.is_supertype(expr_id));

        let subtypes = lang.subtypes(expr_id);
        assert!(!subtypes.is_empty());

        let func_id = lang.resolve_named_node("function_declaration").unwrap();
        assert!(!lang.is_supertype(func_id));
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn field_validation_via_trait() {
        let lang = javascript();

        let func_id = lang.resolve_named_node("function_declaration").unwrap();
        let name_field = lang.resolve_field("name").unwrap();
        let body_field = lang.resolve_field("body").unwrap();

        assert!(lang.has_field(func_id, name_field));
        assert!(lang.has_field(func_id, body_field));

        let identifier_id = lang.resolve_named_node("identifier").unwrap();
        assert!(lang.is_valid_field_type(func_id, name_field, identifier_id));

        let statement_block_id = lang.resolve_named_node("statement_block").unwrap();
        assert!(lang.is_valid_field_type(func_id, body_field, statement_block_id));
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn root_via_trait() {
        let lang = javascript();
        let root_id = lang.root();
        assert!(root_id.is_some());

        let program_id = lang.resolve_named_node("program");
        assert_eq!(root_id, program_id);
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn unresolved_returns_none() {
        let lang = javascript();

        assert!(lang.resolve_named_node("nonexistent_node_type").is_none());
        assert!(lang.resolve_field("nonexistent_field").is_none());
    }

    #[test]
    #[cfg(feature = "rust")]
    fn rust_lang_works() {
        let lang = rust();
        let func_id = lang.resolve_named_node("function_item");
        assert!(func_id.is_some());
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn tree_sitter_id_zero_disambiguation() {
        let lang = javascript();

        // For named nodes: 0 unambiguously means "not found"
        assert!(lang.resolve_named_node("fake_named").is_none());

        // For anonymous nodes: we disambiguate via reverse lookup
        let end_resolved = lang.resolve_anonymous_node("end");
        let fake_resolved = lang.resolve_anonymous_node("totally_fake_node");

        assert!(end_resolved.is_some(), "Valid 'end' node should resolve");
        assert_eq!(end_resolved, Some(0), "'end' should have ID 0");

        assert!(fake_resolved.is_none(), "Non-existent node should be None");

        // Our wrapper preserves field cleanliness
        assert!(lang.resolve_field("name").is_some());
        assert!(lang.resolve_field("fake_field").is_none());
    }

    /// Verifies that languages with "end" keyword assign it a non-zero ID.
    /// This proves that ID 0 ("end" sentinel) is internal to tree-sitter
    /// and never exposed via the Cursor API for actual syntax nodes.
    #[test]
    #[cfg(all(feature = "ruby", feature = "lua"))]
    fn end_keyword_has_nonzero_id() {
        // Ruby has "end" keyword for blocks, methods, classes, etc.
        let ruby = ruby();
        let ruby_end = ruby.resolve_anonymous_node("end");
        assert!(ruby_end.is_some(), "Ruby should have 'end' keyword");
        assert_ne!(ruby_end, Some(0), "Ruby 'end' keyword must not be ID 0");

        // Lua has "end" keyword for blocks, functions, etc.
        let lua = lua();
        let lua_end = lua.resolve_anonymous_node("end");
        assert!(lua_end.is_some(), "Lua should have 'end' keyword");
        assert_ne!(lua_end, Some(0), "Lua 'end' keyword must not be ID 0");

        // Both languages still have internal "end" sentinel at ID 0
        assert_eq!(ruby.node_type_name(0), Some("end"));
        assert_eq!(lua.node_type_name(0), Some("end"));
    }
}
