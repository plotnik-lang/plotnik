use std::sync::LazyLock;

use tree_sitter::Language;

pub use plotnik_core::{Cardinality, NodeFieldId, NodeTypeId, NodeTypes, StaticNodeTypes};

/// Trait providing a unified facade for tree-sitter's Language API
/// combined with our node type constraints.
///
/// Methods that return Option types handle resolution failures gracefully.
pub trait Lang: Send + Sync {
    fn name(&self) -> &str;

    /// Raw tree-sitter Language. You probably don't need this.
    fn get_inner(&self) -> &Language;

    // ═══════════════════════════════════════════════════════════════════════
    // Resolution                                                [Language API]
    // ═══════════════════════════════════════════════════════════════════════

    fn resolve_node(&self, kind: &str, named: bool) -> Option<NodeTypeId>;
    fn resolve_field(&self, name: &str) -> Option<NodeFieldId>;

    // ═══════════════════════════════════════════════════════════════════════
    // Supertype info                                            [Language API]
    // ═══════════════════════════════════════════════════════════════════════

    fn is_supertype(&self, id: Option<NodeTypeId>) -> bool;
    fn subtypes(&self, supertype: Option<NodeTypeId>) -> &[u16];

    // ═══════════════════════════════════════════════════════════════════════
    // Root & Extras                                               [node_types]
    // ═══════════════════════════════════════════════════════════════════════

    fn root(&self) -> Option<NodeTypeId>;
    fn is_extra(&self, id: Option<NodeTypeId>) -> bool;

    // ═══════════════════════════════════════════════════════════════════════
    // Field constraints                                           [node_types]
    // ═══════════════════════════════════════════════════════════════════════

    fn has_field(&self, node: Option<NodeTypeId>, field: Option<NodeFieldId>) -> bool;
    fn field_cardinality(
        &self,
        node: Option<NodeTypeId>,
        field: Option<NodeFieldId>,
    ) -> Option<Cardinality>;
    fn valid_field_types(
        &self,
        node: Option<NodeTypeId>,
        field: Option<NodeFieldId>,
    ) -> &'static [u16];
    fn is_valid_field_type(
        &self,
        node: Option<NodeTypeId>,
        field: Option<NodeFieldId>,
        child: Option<NodeTypeId>,
    ) -> bool;

    // ═══════════════════════════════════════════════════════════════════════
    // Children constraints                                        [node_types]
    // ═══════════════════════════════════════════════════════════════════════

    fn children_cardinality(&self, node: Option<NodeTypeId>) -> Option<Cardinality>;
    fn valid_child_types(&self, node: Option<NodeTypeId>) -> &'static [u16];
    fn is_valid_child_type(&self, node: Option<NodeTypeId>, child: Option<NodeTypeId>) -> bool;
}

/// Static implementation of `Lang` with compile-time generated node types.
#[derive(Debug)]
pub struct StaticLang {
    pub name: &'static str,
    inner: Language,
    node_types: &'static StaticNodeTypes,
}

impl StaticLang {
    pub const fn new(
        name: &'static str,
        inner: Language,
        node_types: &'static StaticNodeTypes,
    ) -> Self {
        Self {
            name,
            inner,
            node_types,
        }
    }

    pub fn node_types(&self) -> &'static StaticNodeTypes {
        self.node_types
    }
}

impl Lang for StaticLang {
    fn name(&self) -> &str {
        self.name
    }

    fn get_inner(&self) -> &Language {
        &self.inner
    }

    fn resolve_node(&self, kind: &str, named: bool) -> Option<NodeTypeId> {
        let id = self.inner.id_for_node_kind(kind, named);

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
                // Named node with ID 0 = definitely not found
                None
            } else {
                // Anonymous node with ID 0 = could be "end" or not found
                // Check via reverse lookup
                if self.inner.node_kind_for_id(0) == Some(kind) {
                    Some(0) // It's the "end" node
                } else {
                    None // Not found
                }
            }
        } else {
            Some(id)
        }
    }

    fn resolve_field(&self, name: &str) -> Option<NodeFieldId> {
        self.inner.field_id_for_name(name)
    }

    fn is_supertype(&self, id: Option<NodeTypeId>) -> bool {
        let Some(raw) = id else { return false };
        self.inner.node_kind_is_supertype(raw)
    }

    fn subtypes(&self, supertype: Option<NodeTypeId>) -> &[u16] {
        let Some(raw) = supertype else {
            return &[];
        };
        self.inner.subtypes_for_supertype(raw)
    }

    fn root(&self) -> Option<NodeTypeId> {
        self.node_types.root()
    }

    fn is_extra(&self, id: Option<NodeTypeId>) -> bool {
        let Some(id) = id else { return false };
        self.node_types.is_extra(id)
    }

    fn has_field(&self, node: Option<NodeTypeId>, field: Option<NodeFieldId>) -> bool {
        let (Some(n), Some(f)) = (node, field) else {
            return false;
        };
        self.node_types.has_field(n, f)
    }

    fn field_cardinality(
        &self,
        node: Option<NodeTypeId>,
        field: Option<NodeFieldId>,
    ) -> Option<Cardinality> {
        let (n, f) = (node?, field?);
        self.node_types.field_cardinality(n, f)
    }

    fn valid_field_types(
        &self,
        node: Option<NodeTypeId>,
        field: Option<NodeFieldId>,
    ) -> &'static [u16] {
        let (Some(n), Some(f)) = (node, field) else {
            return &[];
        };
        self.node_types.valid_field_types(n, f)
    }

    fn is_valid_field_type(
        &self,
        node: Option<NodeTypeId>,
        field: Option<NodeFieldId>,
        child: Option<NodeTypeId>,
    ) -> bool {
        let (Some(n), Some(f), Some(c)) = (node, field, child) else {
            return false;
        };
        self.node_types.is_valid_field_type(n, f, c)
    }

    fn children_cardinality(&self, node: Option<NodeTypeId>) -> Option<Cardinality> {
        let n = node?;
        self.node_types.children_cardinality(n)
    }

    fn valid_child_types(&self, node: Option<NodeTypeId>) -> &'static [u16] {
        let Some(n) = node else { return &[] };
        self.node_types.valid_child_types(n)
    }

    fn is_valid_child_type(&self, node: Option<NodeTypeId>, child: Option<NodeTypeId>) -> bool {
        let (Some(n), Some(c)) = (node, child) else {
            return false;
        };
        self.node_types.is_valid_child_type(n, c)
    }
}

macro_rules! define_langs {
    (
        $(
            $fn_name:ident => {
                feature: $feature:literal,
                name: $name:literal,
                ts_lang: $ts_lang:expr,
                node_types_key: $node_types_key:literal,
                names: [$($alias:literal),* $(,)?],
                extensions: [$($ext:literal),* $(,)?] $(,)?
            }
        ),* $(,)?
    ) => {
        // Generate NodeTypes statics via proc macro
        $(
            #[cfg(feature = $feature)]
            plotnik_macros::generate_node_types!($node_types_key);
        )*

        // Generate static Lang definitions with LazyLock
        $(
            #[cfg(feature = $feature)]
            pub fn $fn_name() -> &'static dyn Lang {
                paste::paste! {
                    static LANG: LazyLock<StaticLang> = LazyLock::new(|| {
                        StaticLang::new(
                            $name,
                            $ts_lang.into(),
                            &[<$node_types_key:upper _NODE_TYPES>],
                        )
                    });
                }
                &*LANG
            }
        )*

        pub fn from_name(s: &str) -> Option<&'static dyn Lang> {
            match s.to_ascii_lowercase().as_str() {
                $(
                    #[cfg(feature = $feature)]
                    $($alias)|* => Some($fn_name()),
                )*
                _ => None,
            }
        }

        pub fn from_ext(ext: &str) -> Option<&'static dyn Lang> {
            match ext.to_ascii_lowercase().as_str() {
                $(
                    #[cfg(feature = $feature)]
                    $($ext)|* => Some($fn_name()),
                )*
                _ => None,
            }
        }

        pub fn all() -> Vec<&'static dyn Lang> {
            vec![
                $(
                    #[cfg(feature = $feature)]
                    $fn_name(),
                )*
            ]
        }
    };
}

define_langs! {
    bash => {
        feature: "bash",
        name: "bash",
        ts_lang: tree_sitter_bash::LANGUAGE,
        node_types_key: "bash",
        names: ["bash", "sh", "shell"],
        extensions: ["sh", "bash", "zsh"],
    },
    c => {
        feature: "c",
        name: "c",
        ts_lang: tree_sitter_c::LANGUAGE,
        node_types_key: "c",
        names: ["c"],
        extensions: ["c", "h"],
    },
    cpp => {
        feature: "cpp",
        name: "cpp",
        ts_lang: tree_sitter_cpp::LANGUAGE,
        node_types_key: "cpp",
        names: ["cpp", "c++", "cxx", "cc"],
        extensions: ["cpp", "cc", "cxx", "hpp", "hh", "hxx", "h++", "c++"],
    },
    csharp => {
        feature: "csharp",
        name: "c_sharp",
        ts_lang: tree_sitter_c_sharp::LANGUAGE,
        node_types_key: "csharp",
        names: ["csharp", "c#", "cs", "c_sharp"],
        extensions: ["cs"],
    },
    css => {
        feature: "css",
        name: "css",
        ts_lang: tree_sitter_css::LANGUAGE,
        node_types_key: "css",
        names: ["css"],
        extensions: ["css"],
    },
    elixir => {
        feature: "elixir",
        name: "elixir",
        ts_lang: tree_sitter_elixir::LANGUAGE,
        node_types_key: "elixir",
        names: ["elixir", "ex"],
        extensions: ["ex", "exs"],
    },
    go => {
        feature: "go",
        name: "go",
        ts_lang: tree_sitter_go::LANGUAGE,
        node_types_key: "go",
        names: ["go", "golang"],
        extensions: ["go"],
    },
    haskell => {
        feature: "haskell",
        name: "haskell",
        ts_lang: tree_sitter_haskell::LANGUAGE,
        node_types_key: "haskell",
        names: ["haskell", "hs"],
        extensions: ["hs", "lhs"],
    },
    hcl => {
        feature: "hcl",
        name: "hcl",
        ts_lang: tree_sitter_hcl::LANGUAGE,
        node_types_key: "hcl",
        names: ["hcl", "terraform", "tf"],
        extensions: ["hcl", "tf", "tfvars"],
    },
    html => {
        feature: "html",
        name: "html",
        ts_lang: tree_sitter_html::LANGUAGE,
        node_types_key: "html",
        names: ["html", "htm"],
        extensions: ["html", "htm"],
    },
    java => {
        feature: "java",
        name: "java",
        ts_lang: tree_sitter_java::LANGUAGE,
        node_types_key: "java",
        names: ["java"],
        extensions: ["java"],
    },
    javascript => {
        feature: "javascript",
        name: "javascript",
        ts_lang: tree_sitter_javascript::LANGUAGE,
        node_types_key: "javascript",
        names: ["javascript", "js", "jsx", "ecmascript", "es"],
        extensions: ["js", "mjs", "cjs", "jsx"],
    },
    json => {
        feature: "json",
        name: "json",
        ts_lang: tree_sitter_json::LANGUAGE,
        node_types_key: "json",
        names: ["json"],
        extensions: ["json"],
    },
    kotlin => {
        feature: "kotlin",
        name: "kotlin",
        ts_lang: tree_sitter_kotlin::LANGUAGE,
        node_types_key: "kotlin",
        names: ["kotlin", "kt"],
        extensions: ["kt", "kts"],
    },
    lua => {
        feature: "lua",
        name: "lua",
        ts_lang: tree_sitter_lua::LANGUAGE,
        node_types_key: "lua",
        names: ["lua"],
        extensions: ["lua"],
    },
    nix => {
        feature: "nix",
        name: "nix",
        ts_lang: tree_sitter_nix::LANGUAGE,
        node_types_key: "nix",
        names: ["nix"],
        extensions: ["nix"],
    },
    php => {
        feature: "php",
        name: "php",
        ts_lang: tree_sitter_php::LANGUAGE_PHP,
        node_types_key: "php",
        names: ["php"],
        extensions: ["php"],
    },
    python => {
        feature: "python",
        name: "python",
        ts_lang: tree_sitter_python::LANGUAGE,
        node_types_key: "python",
        names: ["python", "py"],
        extensions: ["py", "pyi", "pyw"],
    },
    ruby => {
        feature: "ruby",
        name: "ruby",
        ts_lang: tree_sitter_ruby::LANGUAGE,
        node_types_key: "ruby",
        names: ["ruby", "rb"],
        extensions: ["rb", "rake", "gemspec"],
    },
    rust => {
        feature: "rust",
        name: "rust",
        ts_lang: tree_sitter_rust::LANGUAGE,
        node_types_key: "rust",
        names: ["rust", "rs"],
        extensions: ["rs"],
    },
    scala => {
        feature: "scala",
        name: "scala",
        ts_lang: tree_sitter_scala::LANGUAGE,
        node_types_key: "scala",
        names: ["scala"],
        extensions: ["scala", "sc"],
    },
    solidity => {
        feature: "solidity",
        name: "solidity",
        ts_lang: tree_sitter_solidity::LANGUAGE,
        node_types_key: "solidity",
        names: ["solidity", "sol"],
        extensions: ["sol"],
    },
    swift => {
        feature: "swift",
        name: "swift",
        ts_lang: tree_sitter_swift::LANGUAGE,
        node_types_key: "swift",
        names: ["swift"],
        extensions: ["swift"],
    },
    typescript => {
        feature: "typescript",
        name: "typescript",
        ts_lang: tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        node_types_key: "typescript",
        names: ["typescript", "ts"],
        extensions: ["ts", "mts", "cts"],
    },
    tsx => {
        feature: "typescript",
        name: "tsx",
        ts_lang: tree_sitter_typescript::LANGUAGE_TSX,
        node_types_key: "typescript_tsx",
        names: ["tsx"],
        extensions: ["tsx"],
    },
    yaml => {
        feature: "yaml",
        name: "yaml",
        ts_lang: tree_sitter_yaml::LANGUAGE,
        node_types_key: "yaml",
        names: ["yaml", "yml"],
        extensions: ["yaml", "yml"],
    },
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

        let expr_id = lang.resolve_node("expression", true);
        assert!(lang.is_supertype(expr_id));

        let subtypes = lang.subtypes(expr_id);
        assert!(!subtypes.is_empty());

        let func_id = lang.resolve_node("function_declaration", true);
        assert!(!lang.is_supertype(func_id));
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn field_validation_via_trait() {
        let lang = javascript();

        let func_id = lang.resolve_node("function_declaration", true);
        let name_field = lang.resolve_field("name");
        let body_field = lang.resolve_field("body");

        assert!(lang.has_field(func_id, name_field));
        assert!(lang.has_field(func_id, body_field));

        let identifier_id = lang.resolve_node("identifier", true);
        assert!(lang.is_valid_field_type(func_id, name_field, identifier_id));

        let statement_block_id = lang.resolve_node("statement_block", true);
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
    fn unresolved_returns_sensible_defaults() {
        let lang = javascript();

        let unresolved_node: Option<NodeTypeId> = None;
        let unresolved_field: Option<NodeFieldId> = None;

        assert!(!lang.is_supertype(unresolved_node));
        assert!(!lang.is_extra(unresolved_node));
        assert!(!lang.has_field(unresolved_node, unresolved_field));
        assert!(lang.subtypes(unresolved_node).is_empty());
        assert!(
            lang.valid_field_types(unresolved_node, unresolved_field)
                .is_empty()
        );
        assert!(lang.valid_child_types(unresolved_node).is_empty());
        assert!(!lang.is_valid_field_type(unresolved_node, unresolved_field, unresolved_node));
        assert!(!lang.is_valid_child_type(unresolved_node, unresolved_node));
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
    /// This test shows:
    /// - The ambiguity in the raw tree-sitter API
    /// - How our wrapper resolves it correctly
    #[test]
    #[cfg(feature = "javascript")]
    fn tree_sitter_id_zero_ambiguity() {
        let lang = javascript();
        let raw_lang = lang.get_inner();

        // === Part 1: Understanding the problem ===

        // ID 0 is the "end" sentinel node (anonymous)
        assert_eq!(raw_lang.node_kind_for_id(0), Some("end"));
        assert!(!raw_lang.node_kind_is_named(0));

        // Tree-sitter returns 0 for BOTH valid "end" and non-existent nodes
        let end_id = raw_lang.id_for_node_kind("end", false);
        let fake_id = raw_lang.id_for_node_kind("totally_fake_node", false);
        assert_eq!(end_id, 0, "Valid 'end' node returns 0");
        assert_eq!(fake_id, 0, "Non-existent node also returns 0!");

        // This ambiguity doesn't exist for named nodes (0 always = not found)
        let fake_named = raw_lang.id_for_node_kind("fake_named", true);
        assert_eq!(fake_named, 0, "Non-existent named node returns 0");
        // And no named node has ID 0
        assert!(!raw_lang.node_kind_is_named(0));

        // === Part 2: Our wrapper's solution ===

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

        // === Part 3: Field IDs don't have this problem ===

        // Tree-sitter uses Option<NonZeroU16> for fields - clean API!
        let name_field_id = raw_lang.field_id_for_name("name");
        assert!(name_field_id.is_some(), "Field 'name' should exist");
        assert!(name_field_id.unwrap().get() > 0, "Field IDs start at 1");
        assert_eq!(raw_lang.field_id_for_name("fake_field"), None);

        // Our wrapper preserves this cleanliness
        assert!(lang.resolve_field("name").is_some());
        assert!(lang.resolve_field("fake_field").is_none());
    }

    /// Additional test showing the tree-sitter oddities in detail
    #[test]
    #[cfg(feature = "javascript")]
    fn tree_sitter_api_roundtrip_quirks() {
        let lang = javascript();
        let raw_lang = lang.get_inner();

        // Some nodes appear at multiple IDs!
        // This happens when the same node type is used in different contexts
        let mut id_to_names = std::collections::HashMap::<u16, Vec<(&str, bool)>>::new();

        for id in 0..raw_lang.node_kind_count() as u16 {
            if let Some(name) = raw_lang.node_kind_for_id(id) {
                let is_named = raw_lang.node_kind_is_named(id);
                id_to_names.entry(id).or_default().push((name, is_named));

                // The roundtrip might NOT preserve the ID!
                let resolved_id = raw_lang.id_for_node_kind(name, is_named);

                // For example, "identifier" might be at both ID 1 and ID 46,
                // but id_for_node_kind("identifier", true) returns only one of them
                if resolved_id != id && name != "ERROR" {
                    // This is normal - tree-sitter returns the first matching ID
                    // when multiple IDs have the same (name, is_named) combination
                }
            }
        }

        // Verify our assumptions about ID 0
        assert_eq!(id_to_names[&0], vec![("end", false)]);

        // Field IDs are cleaner - they start at 1 (NonZeroU16)
        assert!(raw_lang.field_name_for_id(0).is_none());

        for fid in 1..=raw_lang.field_count() as u16 {
            if let Some(name) = raw_lang.field_name_for_id(fid) {
                // Field roundtrip is reliable
                let resolved = raw_lang.field_id_for_name(name);
                assert_eq!(resolved, std::num::NonZeroU16::new(fid));
            }
        }
    }
}
