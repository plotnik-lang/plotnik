use indexmap::IndexMap;

use crate::infer::{
    OptionalStyle, TypeKey, TypeScriptEmitConfig, TypeTable, TypeValue,
    emit::typescript::{
        dependencies, emit_field_type, emit_inline_struct, topological_sort, wrap_if_union,
    },
    emit_typescript,
};

#[test]
fn emit_empty_table() {
    let table = TypeTable::new();
    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);
    assert_eq!(output, "");
}

#[test]
fn emit_simple_interface() {
    let mut table = TypeTable::new();
    let mut fields = IndexMap::new();
    fields.insert("name", TypeKey::String);
    fields.insert("node", TypeKey::Node);
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(fields));

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface Foo {
          name: string;
          node: SyntaxNode;
        }
        ");
}

#[test]
fn emit_empty_interface() {
    let mut table = TypeTable::new();
    table.insert(TypeKey::Named("Empty"), TypeValue::Struct(IndexMap::new()));

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @"export interface Empty {}");
}

#[test]
fn emit_unit_field() {
    let mut table = TypeTable::new();
    let mut fields = IndexMap::new();
    fields.insert("marker", TypeKey::Unit);
    table.insert(TypeKey::Named("WithUnit"), TypeValue::Struct(fields));

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface WithUnit {
          marker: {};
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

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r#"
        export type Stmt =
          | { tag: "Assign"; target: string }
          | { tag: "Call"; func: string };
        "#);
}

#[test]
fn emit_tagged_union_empty_variant() {
    let mut table = TypeTable::new();
    let mut variants = IndexMap::new();

    variants.insert("None", IndexMap::new());

    let mut some_fields = IndexMap::new();
    some_fields.insert("value", TypeKey::Node);
    variants.insert("Some", some_fields);

    table.insert(TypeKey::Named("Maybe"), TypeValue::TaggedUnion(variants));

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r#"
        export type Maybe =
          | { tag: "None" }
          | { tag: "Some"; value: SyntaxNode };
        "#);
}

#[test]
fn emit_optional_null() {
    let mut table = TypeTable::new();
    table.insert(
        TypeKey::Synthetic(vec!["Foo", "bar"]),
        TypeValue::Optional(TypeKey::Node),
    );

    let mut fields = IndexMap::new();
    fields.insert("bar", TypeKey::Synthetic(vec!["Foo", "bar"]));
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(fields));

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface Foo {
          bar: SyntaxNode | null;
        }
        ");
}

#[test]
fn emit_optional_undefined() {
    let mut table = TypeTable::new();
    table.insert(
        TypeKey::Synthetic(vec!["Foo", "bar"]),
        TypeValue::Optional(TypeKey::Node),
    );

    let mut fields = IndexMap::new();
    fields.insert("bar", TypeKey::Synthetic(vec!["Foo", "bar"]));
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(fields));

    let config = TypeScriptEmitConfig {
        optional_style: OptionalStyle::Undefined,
        ..Default::default()
    };
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface Foo {
          bar: SyntaxNode | undefined;
        }
        ");
}

#[test]
fn emit_optional_question_mark() {
    let mut table = TypeTable::new();
    table.insert(
        TypeKey::Synthetic(vec!["Foo", "bar"]),
        TypeValue::Optional(TypeKey::Node),
    );

    let mut fields = IndexMap::new();
    fields.insert("bar", TypeKey::Synthetic(vec!["Foo", "bar"]));
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(fields));

    let config = TypeScriptEmitConfig {
        optional_style: OptionalStyle::QuestionMark,
        ..Default::default()
    };
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface Foo {
          bar?: SyntaxNode;
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

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface Foo {
          items: SyntaxNode[];
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

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface Foo {
          items: [string, ...string[]];
        }
        ");
}

#[test]
fn emit_nested_interface() {
    let mut table = TypeTable::new();

    let mut inner_fields = IndexMap::new();
    inner_fields.insert("value", TypeKey::String);
    table.insert(TypeKey::Named("Inner"), TypeValue::Struct(inner_fields));

    let mut outer_fields = IndexMap::new();
    outer_fields.insert("inner", TypeKey::Named("Inner"));
    table.insert(TypeKey::Named("Outer"), TypeValue::Struct(outer_fields));

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface Inner {
          value: string;
        }

        export interface Outer {
          inner: Inner;
        }
        ");
}

#[test]
fn emit_inline_synthetic() {
    let mut table = TypeTable::new();

    let mut inner_fields = IndexMap::new();
    inner_fields.insert("x", TypeKey::Node);
    table.insert(
        TypeKey::Synthetic(vec!["Foo", "bar"]),
        TypeValue::Struct(inner_fields),
    );

    let mut outer_fields = IndexMap::new();
    outer_fields.insert("bar", TypeKey::Synthetic(vec!["Foo", "bar"]));
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(outer_fields));

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface Foo {
          bar: { x: SyntaxNode };
        }
        ");
}

#[test]
fn emit_no_inline_synthetic() {
    let mut table = TypeTable::new();

    let mut inner_fields = IndexMap::new();
    inner_fields.insert("x", TypeKey::Node);
    table.insert(
        TypeKey::Synthetic(vec!["Foo", "bar"]),
        TypeValue::Struct(inner_fields),
    );

    let mut outer_fields = IndexMap::new();
    outer_fields.insert("bar", TypeKey::Synthetic(vec!["Foo", "bar"]));
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(outer_fields));

    let config = TypeScriptEmitConfig {
        inline_synthetic: false,
        ..Default::default()
    };
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface FooBar {
          x: SyntaxNode;
        }

        export interface Foo {
          bar: FooBar;
        }
        ");
}

#[test]
fn emit_readonly_fields() {
    let mut table = TypeTable::new();
    let mut fields = IndexMap::new();
    fields.insert("name", TypeKey::String);
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(fields));

    let config = TypeScriptEmitConfig {
        readonly: true,
        ..Default::default()
    };
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface Foo {
          readonly name: string;
        }
        ");
}

#[test]
fn emit_no_export() {
    let mut table = TypeTable::new();
    table.insert(
        TypeKey::Named("Private"),
        TypeValue::Struct(IndexMap::new()),
    );

    let config = TypeScriptEmitConfig {
        export: false,
        ..Default::default()
    };
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @"interface Private {}");
}

#[test]
fn emit_custom_node_type() {
    let mut table = TypeTable::new();
    let mut fields = IndexMap::new();
    fields.insert("node", TypeKey::Node);
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(fields));

    let config = TypeScriptEmitConfig {
        node_type_name: "TSNode".to_string(),
        ..Default::default()
    };
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface Foo {
          node: TSNode;
        }
        ");
}

#[test]
fn emit_cyclic_type_no_box() {
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

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    // TypeScript handles cycles natively, no Box needed
    insta::assert_snapshot!(output, @r"
        export interface Tree {
          value: string;
          child: Tree | null;
        }
        ");
}

#[test]
fn emit_list_of_optional() {
    let mut table = TypeTable::new();
    table.insert(
        TypeKey::Synthetic(vec!["Foo", "inner"]),
        TypeValue::Optional(TypeKey::Node),
    );
    table.insert(
        TypeKey::Synthetic(vec!["Foo", "items"]),
        TypeValue::List(TypeKey::Synthetic(vec!["Foo", "inner"])),
    );

    let mut fields = IndexMap::new();
    fields.insert("items", TypeKey::Synthetic(vec!["Foo", "items"]));
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(fields));

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface Foo {
          items: (SyntaxNode | null)[];
        }
        ");
}

#[test]
fn emit_deeply_nested_inline() {
    let mut table = TypeTable::new();

    let mut level2 = IndexMap::new();
    level2.insert("val", TypeKey::String);
    table.insert(
        TypeKey::Synthetic(vec!["A", "b", "c"]),
        TypeValue::Struct(level2),
    );

    let mut level1 = IndexMap::new();
    level1.insert("c", TypeKey::Synthetic(vec!["A", "b", "c"]));
    table.insert(
        TypeKey::Synthetic(vec!["A", "b"]),
        TypeValue::Struct(level1),
    );

    let mut root = IndexMap::new();
    root.insert("b", TypeKey::Synthetic(vec!["A", "b"]));
    table.insert(TypeKey::Named("A"), TypeValue::Struct(root));

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface A {
          b: { c: { val: string } };
        }
        ");
}

#[test]
fn emit_type_alias_when_not_inlined() {
    let mut table = TypeTable::new();
    table.insert(
        TypeKey::Named("OptionalNode"),
        TypeValue::Optional(TypeKey::Node),
    );

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @"export type OptionalNode = SyntaxNode | null;");
}

#[test]
fn emit_type_alias_list() {
    let mut table = TypeTable::new();
    table.insert(TypeKey::Named("NodeList"), TypeValue::List(TypeKey::Node));

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @"export type NodeList = SyntaxNode[];");
}

#[test]
fn emit_type_alias_non_empty_list() {
    let mut table = TypeTable::new();
    table.insert(
        TypeKey::Named("NonEmptyNodes"),
        TypeValue::NonEmptyList(TypeKey::Node),
    );

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @"export type NonEmptyNodes = [SyntaxNode, ...SyntaxNode[]];");
}

#[test]
fn wrap_if_union_simple() {
    assert_eq!(wrap_if_union("string"), "string");
    assert_eq!(wrap_if_union("SyntaxNode"), "SyntaxNode");
}

#[test]
fn wrap_if_union_with_pipe() {
    assert_eq!(wrap_if_union("string | null"), "(string | null)");
    assert_eq!(wrap_if_union("A | B | C"), "(A | B | C)");
}

#[test]
fn inline_empty_struct() {
    let fields = IndexMap::new();
    let table = TypeTable::new();
    let config = TypeScriptEmitConfig::default();
    let result = emit_inline_struct(&fields, &table, &config);
    assert_eq!(result, "{}");
}

#[test]
fn inline_struct_multiple_fields() {
    let mut fields = IndexMap::new();
    fields.insert("a", TypeKey::String);
    fields.insert("b", TypeKey::Node);
    let table = TypeTable::new();
    let config = TypeScriptEmitConfig::default();
    let result = emit_inline_struct(&fields, &table, &config);
    assert_eq!(result, "{ a: string; b: SyntaxNode }");
}

#[test]
fn dependencies_primitives() {
    assert!(dependencies(&TypeValue::Node).is_empty());
    assert!(dependencies(&TypeValue::String).is_empty());
    assert!(dependencies(&TypeValue::Unit).is_empty());
}

#[test]
fn dependencies_struct() {
    let mut fields = IndexMap::new();
    fields.insert("a", TypeKey::Named("A"));
    fields.insert("b", TypeKey::Named("B"));
    let value = TypeValue::Struct(fields);

    let deps = dependencies(&value);
    assert_eq!(deps.len(), 2);
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
fn optional_style_equality() {
    assert_eq!(OptionalStyle::Null, OptionalStyle::Null);
    assert_ne!(OptionalStyle::Null, OptionalStyle::Undefined);
    assert_ne!(OptionalStyle::Undefined, OptionalStyle::QuestionMark);
}

#[test]
fn config_default() {
    let config = TypeScriptEmitConfig::default();
    assert_eq!(config.optional_style, OptionalStyle::Null);
    assert!(config.export);
    assert!(!config.readonly);
    assert!(config.inline_synthetic);
    assert_eq!(config.node_type_name, "SyntaxNode");
}

#[test]
fn emit_tagged_union_optional_field_question() {
    let mut table = TypeTable::new();

    table.insert(
        TypeKey::Synthetic(vec!["Stmt", "x"]),
        TypeValue::Optional(TypeKey::Node),
    );

    let mut variants = IndexMap::new();
    let mut v_fields = IndexMap::new();
    v_fields.insert("x", TypeKey::Synthetic(vec!["Stmt", "x"]));
    variants.insert("V", v_fields);

    table.insert(TypeKey::Named("Stmt"), TypeValue::TaggedUnion(variants));

    let config = TypeScriptEmitConfig {
        optional_style: OptionalStyle::QuestionMark,
        ..Default::default()
    };
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r#"
        export type Stmt =
          | { tag: "V"; x?: SyntaxNode };
        "#);
}

#[test]
fn topological_sort_with_cycle() {
    let mut table = TypeTable::new();

    let mut a_fields = IndexMap::new();
    a_fields.insert("b", TypeKey::Named("B"));
    table.insert(TypeKey::Named("A"), TypeValue::Struct(a_fields));

    let mut b_fields = IndexMap::new();
    b_fields.insert("a", TypeKey::Named("A"));
    table.insert(TypeKey::Named("B"), TypeValue::Struct(b_fields));

    let sorted = topological_sort(&table);
    assert!(sorted.contains(&TypeKey::Named("A")));
    assert!(sorted.contains(&TypeKey::Named("B")));
}

#[test]
fn emit_field_type_unknown_key() {
    let table = TypeTable::new();
    let config = TypeScriptEmitConfig::default();
    let (type_str, is_optional) = emit_field_type(&TypeKey::Named("Unknown"), &table, &config);
    assert_eq!(type_str, "Unknown");
    assert!(!is_optional);
}

#[test]
fn emit_readonly_optional_question_mark() {
    let mut table = TypeTable::new();
    table.insert(
        TypeKey::Synthetic(vec!["Foo", "bar"]),
        TypeValue::Optional(TypeKey::String),
    );

    let mut fields = IndexMap::new();
    fields.insert("bar", TypeKey::Synthetic(vec!["Foo", "bar"]));
    table.insert(TypeKey::Named("Foo"), TypeValue::Struct(fields));

    let config = TypeScriptEmitConfig {
        readonly: true,
        optional_style: OptionalStyle::QuestionMark,
        ..Default::default()
    };
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface Foo {
          readonly bar?: string;
        }
        ");
}

#[test]
fn inline_struct_with_optional_question_mark() {
    let mut table = TypeTable::new();
    table.insert(
        TypeKey::Synthetic(vec!["inner", "opt"]),
        TypeValue::Optional(TypeKey::Node),
    );

    let mut fields = IndexMap::new();
    fields.insert("opt", TypeKey::Synthetic(vec!["inner", "opt"]));

    let config = TypeScriptEmitConfig {
        optional_style: OptionalStyle::QuestionMark,
        ..Default::default()
    };

    let result = emit_inline_struct(&fields, &table, &config);
    assert_eq!(result, "{ opt?: SyntaxNode }");
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
fn topological_sort_missing_dependency() {
    let mut table = TypeTable::new();

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

    let config = TypeScriptEmitConfig::default();
    let output = emit_typescript(&table, &config);

    insta::assert_snapshot!(output, @r"
        export interface Foo {
          ref_field: Missing;
        }
        ");
}
