use indexmap::IndexMap;

use crate::infer::{
    Indirection, RustEmitConfig, TypeKey, TypeTable, TypeValue,
    emit::rust::{dependencies, emit_derives, emit_type_ref, topological_sort, wrap_indirection},
    emit_rust,
};

#[test]
fn emit_empty_table() {
    let table = TypeTable::new();
    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);
    assert_eq!(output, "");
}

#[test]
fn emit_simple_struct() {
    let mut table = TypeTable::new();
    let mut fields = IndexMap::new();
    fields.insert("name", TypeKey::String);
    fields.insert("node", TypeKey::Node);
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(fields));

    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        #[derive(Debug, Clone)]
        pub struct Foo {
            pub name: String,
            pub node: Node,
        }
        ");
}

#[test]
fn emit_empty_struct() {
    let mut table = TypeTable::new();
    table.insert(TypeKey::Named("Empty"), TypeValue::Struct(IndexMap::new()));

    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        #[derive(Debug, Clone)]
        pub struct Empty;
        ");
}

#[test]
fn emit_unit_field() {
    let mut table = TypeTable::new();
    let mut fields = IndexMap::new();
    fields.insert("marker", TypeKey::Unit);
    table.insert(TypeKey::Named("WithUnit"), TypeValue::Struct(fields));

    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        #[derive(Debug, Clone)]
        pub struct WithUnit {
            pub marker: (),
        }
        ");
}

#[test]
fn emit_tagged_union() {
    let mut table = TypeTable::new();
    let mut variants = IndexMap::new();

    let mut assign_fields = IndexMap::new();
    assign_fields.insert("target", TypeKey::String);
    variants.insert("Assign", assign_fields);

    let mut call_fields = IndexMap::new();
    call_fields.insert("func", TypeKey::String);
    variants.insert("Call", call_fields);

    table.insert(TypeKey::Named("Stmt"), TypeValue::TaggedUnion(variants));

    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        #[derive(Debug, Clone)]
        pub enum Stmt {
            Assign {
                target: String,
            },
            Call {
                func: String,
            },
        }
        ");
}

#[test]
fn emit_tagged_union_unit_variant() {
    let mut table = TypeTable::new();
    let mut variants = IndexMap::new();

    variants.insert("None", IndexMap::new());

    let mut some_fields = IndexMap::new();
    some_fields.insert("value", TypeKey::Node);
    variants.insert("Some", some_fields);

    table.insert(TypeKey::Named("Maybe"), TypeValue::TaggedUnion(variants));

    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        #[derive(Debug, Clone)]
        pub enum Maybe {
            None,
            Some {
                value: Node,
            },
        }
        ");
}

#[test]
fn emit_optional_field() {
    let mut table = TypeTable::new();
    table.insert(
        TypeKey::Synthetic(vec!["Foo", "bar"]),
        TypeValue::Optional(TypeKey::Node),
    );

    let mut fields = IndexMap::new();
    fields.insert("bar", TypeKey::Synthetic(vec!["Foo", "bar"]));
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(fields));

    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        pub type FooBar = Option<Node>;

        #[derive(Debug, Clone)]
        pub struct Foo {
            pub bar: Option<Node>,
        }
        ");
}

#[test]
fn emit_list_field() {
    let mut table = TypeTable::new();
    table.insert(
        TypeKey::Synthetic(vec!["Foo", "items"]),
        TypeValue::List(TypeKey::Node),
    );

    let mut fields = IndexMap::new();
    fields.insert("items", TypeKey::Synthetic(vec!["Foo", "items"]));
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(fields));

    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        pub type FooItems = Vec<Node>;

        #[derive(Debug, Clone)]
        pub struct Foo {
            pub items: Vec<Node>,
        }
        ");
}

#[test]
fn emit_non_empty_list_field() {
    let mut table = TypeTable::new();
    table.insert(
        TypeKey::Synthetic(vec!["Foo", "items"]),
        TypeValue::NonEmptyList(TypeKey::String),
    );

    let mut fields = IndexMap::new();
    fields.insert("items", TypeKey::Synthetic(vec!["Foo", "items"]));
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(fields));

    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        pub type FooItems = Vec<String>;

        #[derive(Debug, Clone)]
        pub struct Foo {
            pub items: Vec<String>,
        }
        ");
}

#[test]
fn emit_nested_struct() {
    let mut table = TypeTable::new();

    let mut inner_fields = IndexMap::new();
    inner_fields.insert("value", TypeKey::String);
    table.insert(TypeKey::Named("Inner"), TypeValue::Struct(inner_fields));

    let mut outer_fields = IndexMap::new();
    outer_fields.insert("inner", TypeKey::Named("Inner"));
    table.insert(TypeKey::Named("Outer"), TypeValue::Struct(outer_fields));

    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        #[derive(Debug, Clone)]
        pub struct Inner {
            pub value: String,
        }

        #[derive(Debug, Clone)]
        pub struct Outer {
            pub inner: Inner,
        }
        ");
}

#[test]
fn emit_cyclic_type_box() {
    let mut table = TypeTable::new();

    table.insert(
        TypeKey::Synthetic(vec!["Tree", "child"]),
        TypeValue::Optional(TypeKey::Named("Tree")),
    );

    let mut fields = IndexMap::new();
    fields.insert("value", TypeKey::String);
    fields.insert("child", TypeKey::Synthetic(vec!["Tree", "child"]));
    table.insert(TypeKey::Named("Tree"), TypeValue::Struct(fields));

    table.mark_cyclic(TypeKey::Named("Tree"));

    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        #[derive(Debug, Clone)]
        pub struct Tree {
            pub value: String,
            pub child: Option<Box<Tree>>,
        }

        pub type TreeChild = Option<Box<Tree>>;
        ");
}

#[test]
fn emit_cyclic_type_rc() {
    let mut table = TypeTable::new();

    table.insert(
        TypeKey::Synthetic(vec!["Tree", "child"]),
        TypeValue::Optional(TypeKey::Named("Tree")),
    );

    let mut fields = IndexMap::new();
    fields.insert("child", TypeKey::Synthetic(vec!["Tree", "child"]));
    table.insert(TypeKey::Named("Tree"), TypeValue::Struct(fields));

    table.mark_cyclic(TypeKey::Named("Tree"));

    let config = RustEmitConfig {
        indirection: Indirection::Rc,
        ..Default::default()
    };
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        #[derive(Debug, Clone)]
        pub struct Tree {
            pub child: Option<Rc<Tree>>,
        }

        pub type TreeChild = Option<Rc<Tree>>;
        ");
}

#[test]
fn emit_cyclic_type_arc() {
    let mut table = TypeTable::new();

    let mut fields = IndexMap::new();
    fields.insert("next", TypeKey::Named("Node"));
    table.insert(TypeKey::Named("Node"), TypeValue::Struct(fields));

    table.mark_cyclic(TypeKey::Named("Node"));

    let config = RustEmitConfig {
        indirection: Indirection::Arc,
        ..Default::default()
    };
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        #[derive(Debug, Clone)]
        pub struct Node {
            pub next: Arc<Node>,
        }
        ");
}

#[test]
fn emit_no_derives() {
    let mut table = TypeTable::new();
    table.insert(TypeKey::Named("Plain"), TypeValue::Struct(IndexMap::new()));

    let config = RustEmitConfig {
        indirection: Indirection::Box,
        derive_debug: false,
        derive_clone: false,
        derive_partial_eq: false,
    };
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @"pub struct Plain;");
}

#[test]
fn emit_all_derives() {
    let mut table = TypeTable::new();
    table.insert(TypeKey::Named("Full"), TypeValue::Struct(IndexMap::new()));

    let config = RustEmitConfig {
        indirection: Indirection::Box,
        derive_debug: true,
        derive_clone: true,
        derive_partial_eq: true,
    };
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        #[derive(Debug, Clone, PartialEq)]
        pub struct Full;
        ");
}

#[test]
fn emit_synthetic_type_name() {
    let mut table = TypeTable::new();

    let mut fields = IndexMap::new();
    fields.insert("x", TypeKey::Node);
    table.insert(
        TypeKey::Synthetic(vec!["Foo", "bar", "baz"]),
        TypeValue::Struct(fields),
    );

    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        #[derive(Debug, Clone)]
        pub struct FooBarBaz {
            pub x: Node,
        }
        ");
}

#[test]
fn emit_complex_nested() {
    let mut table = TypeTable::new();

    // Inner struct
    let mut inner = IndexMap::new();
    inner.insert("value", TypeKey::String);
    table.insert(
        TypeKey::Synthetic(vec!["Root", "item"]),
        TypeValue::Struct(inner),
    );

    // List of inner
    table.insert(
        TypeKey::Synthetic(vec!["Root", "items"]),
        TypeValue::List(TypeKey::Synthetic(vec!["Root", "item"])),
    );

    // Root struct
    let mut root = IndexMap::new();
    root.insert("items", TypeKey::Synthetic(vec!["Root", "items"]));
    table.insert(TypeKey::Named("Root"), TypeValue::Struct(root));

    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        #[derive(Debug, Clone)]
        pub struct RootItem {
            pub value: String,
        }

        pub type RootItems = Vec<RootItem>;

        #[derive(Debug, Clone)]
        pub struct Root {
            pub items: Vec<RootItem>,
        }
        ");
}

#[test]
fn emit_optional_list() {
    let mut table = TypeTable::new();

    table.insert(
        TypeKey::Synthetic(vec!["Foo", "items", "inner"]),
        TypeValue::List(TypeKey::Node),
    );
    table.insert(
        TypeKey::Synthetic(vec!["Foo", "items"]),
        TypeValue::Optional(TypeKey::Synthetic(vec!["Foo", "items", "inner"])),
    );

    let mut fields = IndexMap::new();
    fields.insert("items", TypeKey::Synthetic(vec!["Foo", "items"]));
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(fields));

    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);

    insta::assert_snapshot!(output, @r"
        pub type FooItemsInner = Vec<Node>;

        pub type FooItems = Option<Vec<Node>>;

        #[derive(Debug, Clone)]
        pub struct Foo {
            pub items: Option<Vec<Node>>,
        }
        ");
}

#[test]
fn topological_sort_simple() {
    let mut table = TypeTable::new();
    table.insert(TypeKey::Named("A"), TypeValue::Unit);
    table.insert(TypeKey::Named("B"), TypeValue::Unit);

    let sorted = topological_sort(&table);
    let names: Vec<_> = sorted.iter().map(|k| k.to_pascal_case()).collect();

    // Builtins first
    assert!(names.iter().position(|n| n == "Node") < names.iter().position(|n| n == "A"));
}

#[test]
fn topological_sort_with_dependency() {
    let mut table = TypeTable::new();

    let mut b_fields = IndexMap::new();
    b_fields.insert("a", TypeKey::Named("A"));
    table.insert(TypeKey::Named("B"), TypeValue::Struct(b_fields));

    table.insert(TypeKey::Named("A"), TypeValue::Unit);

    let sorted = topological_sort(&table);
    let names: Vec<_> = sorted.iter().map(|k| k.to_pascal_case()).collect();

    let a_pos = names.iter().position(|n| n == "A").unwrap();
    let b_pos = names.iter().position(|n| n == "B").unwrap();
    assert!(a_pos < b_pos, "A should come before B");
}

#[test]
fn topological_sort_cycle() {
    let mut table = TypeTable::new();

    let mut a_fields = IndexMap::new();
    a_fields.insert("b", TypeKey::Named("B"));
    table.insert(TypeKey::Named("A"), TypeValue::Struct(a_fields));

    let mut b_fields = IndexMap::new();
    b_fields.insert("a", TypeKey::Named("A"));
    table.insert(TypeKey::Named("B"), TypeValue::Struct(b_fields));

    // Should not panic
    let sorted = topological_sort(&table);
    assert!(sorted.contains(&TypeKey::Named("A")));
    assert!(sorted.contains(&TypeKey::Named("B")));
}

#[test]
fn dependencies_struct() {
    let mut fields = IndexMap::new();
    fields.insert("a", TypeKey::Named("A"));
    fields.insert("b", TypeKey::Named("B"));
    let value = TypeValue::Struct(fields);

    let deps = dependencies(&value);
    assert_eq!(deps.len(), 2);
    assert!(deps.contains(&TypeKey::Named("A")));
    assert!(deps.contains(&TypeKey::Named("B")));
}

#[test]
fn dependencies_tagged_union() {
    let mut variants = IndexMap::new();
    let mut v1 = IndexMap::new();
    v1.insert("x", TypeKey::Named("X"));
    variants.insert("V1", v1);

    let mut v2 = IndexMap::new();
    v2.insert("y", TypeKey::Named("Y"));
    variants.insert("V2", v2);

    let value = TypeValue::TaggedUnion(variants);
    let deps = dependencies(&value);

    assert_eq!(deps.len(), 2);
    assert!(deps.contains(&TypeKey::Named("X")));
    assert!(deps.contains(&TypeKey::Named("Y")));
}

#[test]
fn dependencies_primitives() {
    assert!(dependencies(&TypeValue::Node).is_empty());
    assert!(dependencies(&TypeValue::String).is_empty());
    assert!(dependencies(&TypeValue::Unit).is_empty());
}

#[test]
fn dependencies_wrappers() {
    let opt = TypeValue::Optional(TypeKey::Named("T"));
    let list = TypeValue::List(TypeKey::Named("T"));
    let ne = TypeValue::NonEmptyList(TypeKey::Named("T"));

    assert_eq!(dependencies(&opt), vec![TypeKey::Named("T")]);
    assert_eq!(dependencies(&list), vec![TypeKey::Named("T")]);
    assert_eq!(dependencies(&ne), vec![TypeKey::Named("T")]);
}

#[test]
fn indirection_equality() {
    assert_eq!(Indirection::Box, Indirection::Box);
    assert_ne!(Indirection::Box, Indirection::Rc);
    assert_ne!(Indirection::Rc, Indirection::Arc);
}

#[test]
fn wrap_indirection_all_variants() {
    assert_eq!(wrap_indirection("Foo", Indirection::Box), "Box<Foo>");
    assert_eq!(wrap_indirection("Foo", Indirection::Rc), "Rc<Foo>");
    assert_eq!(wrap_indirection("Foo", Indirection::Arc), "Arc<Foo>");
}

#[test]
fn emit_derives_partial() {
    let config = RustEmitConfig {
        derive_debug: true,
        derive_clone: false,
        derive_partial_eq: true,
        ..Default::default()
    };
    let derives = emit_derives(&config);
    assert_eq!(derives, "#[derive(Debug, PartialEq)]\n");
}

#[test]
fn emit_type_ref_unknown_key() {
    let table = TypeTable::new();
    let config = RustEmitConfig::default();
    let type_str = emit_type_ref(&TypeKey::Named("Unknown"), &table, &config);
    assert_eq!(type_str, "Unknown");
}

#[test]
fn topological_sort_missing_dependency() {
    let mut table = TypeTable::new();

    // Struct references a type that doesn't exist in the table
    let mut fields = IndexMap::new();
    fields.insert("missing", TypeKey::Named("DoesNotExist"));
    table.insert(TypeKey::Named("HasMissing"), TypeValue::Struct(fields));

    // Should not panic, includes all visited keys
    let sorted = topological_sort(&table);
    assert!(sorted.contains(&TypeKey::Named("HasMissing")));
    // The missing key is visited and added to result (dependency comes before dependent)
    assert!(sorted.contains(&TypeKey::Named("DoesNotExist")));
}

#[test]
fn emit_with_missing_dependency() {
    let mut table = TypeTable::new();

    let mut fields = IndexMap::new();
    fields.insert("ref_field", TypeKey::Named("Missing"));
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(fields));

    let config = RustEmitConfig::default();
    let output = emit_rust(&table, &config);

    // Should emit with the unknown type name
    insta::assert_snapshot!(output, @r"
        #[derive(Debug, Clone)]
        pub struct Foo {
            pub ref_field: Missing,
        }
        ");
}
