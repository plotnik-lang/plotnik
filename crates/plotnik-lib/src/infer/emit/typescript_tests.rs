use super::typescript::{OptionalStyle, TypeScriptEmitConfig, emit_typescript};
use crate::infer::tyton::parse;
use indoc::indoc;

fn emit(input: &str) -> String {
    let table = parse(input).expect("tyton parse failed");
    emit_typescript(&table, &TypeScriptEmitConfig::default())
}

fn emit_with_config(input: &str, config: &TypeScriptEmitConfig) -> String {
    let table = parse(input).expect("tyton parse failed");
    emit_typescript(&table, config)
}

// --- Simple Structs (Interfaces) ---

#[test]
fn emit_interface_single_field() {
    let input = "Foo = { #Node @value }";
    insta::assert_snapshot!(emit(input), @r"
    interface Foo {
      value: SyntaxNode;
    }
    ");
}

#[test]
fn emit_interface_multiple_fields() {
    let input = "Func = { #string @name #Node @body #Node @params }";
    insta::assert_snapshot!(emit(input), @r"
    interface Func {
      name: string;
      body: SyntaxNode;
      params: SyntaxNode;
    }
    ");
}

#[test]
fn emit_interface_empty() {
    let input = "Empty = {}";
    insta::assert_snapshot!(emit(input), @"interface Empty {}");
}

#[test]
fn emit_interface_with_unit_field() {
    let input = "Wrapper = { () @marker }";
    insta::assert_snapshot!(emit(input), @r"
    interface Wrapper {
      marker: {};
    }
    ");
}

#[test]
fn emit_interface_nested_refs() {
    let input = indoc! {r#"
        Inner = { #Node @value }
        Outer = { Inner @inner #string @label }
    "#};
    insta::assert_snapshot!(emit(input), @r"
    interface Inner {
      value: SyntaxNode;
    }

    interface Outer {
      inner: Inner;
      label: string;
    }
    ");
}

// --- Tagged Unions ---

#[test]
fn emit_tagged_union_simple() {
    let input = indoc! {r#"
        AssignStmt = { #Node @target #Node @value }
        CallStmt = { #Node @func }
        Stmt = [ Assign: AssignStmt Call: CallStmt ]
    "#};
    insta::assert_snapshot!(emit(input), @r#"
    interface AssignStmt {
      target: SyntaxNode;
      value: SyntaxNode;
    }

    interface CallStmt {
      func: SyntaxNode;
    }

    type Stmt =
      | { tag: "Assign"; target: SyntaxNode; value: SyntaxNode }
      | { tag: "Call"; func: SyntaxNode };
    "#);
}

#[test]
fn emit_tagged_union_with_empty_variant() {
    let input = indoc! {r#"
        ValueVariant = { #Node @value }
        Expr = [ Some: ValueVariant None: () ]
    "#};
    insta::assert_snapshot!(emit(input), @r#"
    interface ValueVariant {
      value: SyntaxNode;
    }

    type Expr =
      | { tag: "Some"; value: SyntaxNode }
      | { tag: "None" };
    "#);
}

#[test]
fn emit_tagged_union_all_empty() {
    let input = "Token = [ Comma: () Dot: () Semi: () ]";
    insta::assert_snapshot!(emit(input), @r#"
    type Token =
      | { tag: "Comma" }
      | { tag: "Dot" }
      | { tag: "Semi" };
    "#);
}

#[test]
fn emit_tagged_union_with_builtins() {
    let input = "Value = [ Text: #string Code: #Node Empty: () ]";
    insta::assert_snapshot!(emit(input), @r#"
    type Value =
      | { tag: "Text" }
      | { tag: "Code" }
      | { tag: "Empty" };
    "#);
}

// --- Wrapper Types ---

#[test]
fn emit_optional_null() {
    let input = "MaybeNode = #Node?";
    insta::assert_snapshot!(emit(input), @"type MaybeNode = SyntaxNode | null;");
}

#[test]
fn emit_optional_undefined() {
    let input = "MaybeNode = #Node?";
    let config = TypeScriptEmitConfig {
        optional_style: OptionalStyle::Undefined,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @"type MaybeNode = SyntaxNode | undefined;");
}

#[test]
fn emit_optional_question_mark() {
    let input = indoc! {r#"
        MaybeNode = #Node?
        Foo = { MaybeNode @maybe }
    "#};
    let config = TypeScriptEmitConfig {
        optional_style: OptionalStyle::QuestionMark,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @r"
    type MaybeNode = SyntaxNode;

    interface Foo {
      maybe?: SyntaxNode;
    }
    ");
}

#[test]
fn emit_list() {
    let input = "Nodes = #Node*";
    insta::assert_snapshot!(emit(input), @"type Nodes = SyntaxNode[];");
}

#[test]
fn emit_non_empty_list() {
    let input = "Nodes = #Node+";
    insta::assert_snapshot!(emit(input), @"type Nodes = [SyntaxNode, ...SyntaxNode[]];");
}

#[test]
fn emit_optional_named() {
    let input = indoc! {r#"
        Stmt = { #Node @value }
        MaybeStmt = Stmt?
    "#};
    insta::assert_snapshot!(emit(input), @r"
    interface Stmt {
      value: SyntaxNode;
    }

    type MaybeStmt = Stmt | null;
    ");
}

#[test]
fn emit_list_named() {
    let input = indoc! {r#"
        Stmt = { #Node @value }
        Stmts = Stmt*
    "#};
    insta::assert_snapshot!(emit(input), @r"
    interface Stmt {
      value: SyntaxNode;
    }

    type Stmts = Stmt[];
    ");
}

#[test]
fn emit_nested_wrappers() {
    let input = indoc! {r#"
        Item = { #Node @value }
        Items = Item*
        MaybeItems = Items?
    "#};
    insta::assert_snapshot!(emit(input), @r"
    interface Item {
      value: SyntaxNode;
    }

    type Items = Item[];

    type MaybeItems = Item[] | null;
    ");
}

#[test]
fn emit_list_of_optionals() {
    let input = indoc! {r#"
        Item = { #Node @value }
        MaybeItem = Item?
        Items = MaybeItem*
    "#};
    insta::assert_snapshot!(emit(input), @r"
    interface Item {
      value: SyntaxNode;
    }

    type MaybeItem = Item | null;

    type Items = (Item | null)[];
    ");
}

// --- Config Variations ---

#[test]
fn emit_with_export() {
    let input = "Foo = { #Node @value }";
    let config = TypeScriptEmitConfig {
        export: true,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @r"
    export interface Foo {
      value: SyntaxNode;
    }
    ");
}

#[test]
fn emit_readonly_fields() {
    let input = "Foo = { #Node @value #string @name }";
    let config = TypeScriptEmitConfig {
        readonly: true,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @r"
    interface Foo {
      readonly value: SyntaxNode;
      readonly name: string;
    }
    ");
}

#[test]
fn emit_custom_node_type() {
    let input = "Foo = { #Node @value }";
    let config = TypeScriptEmitConfig {
        node_type_name: "TSNode".to_string(),
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @r"
    interface Foo {
      value: TSNode;
    }
    ");
}

#[test]
fn emit_type_alias_instead_of_interface() {
    let input = "Foo = { #Node @value #string @name }";
    let config = TypeScriptEmitConfig {
        use_type_alias: true,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @"type Foo = { value: SyntaxNode; name: string };");
}

#[test]
fn emit_type_alias_empty() {
    let input = "Empty = {}";
    let config = TypeScriptEmitConfig {
        use_type_alias: true,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @"type Empty = {};");
}

#[test]
fn emit_type_alias_nested() {
    let input = indoc! {r#"
        Inner = { #Node @value }
        Outer = { Inner @inner #string @label }
    "#};
    let config = TypeScriptEmitConfig {
        use_type_alias: true,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @r"
    type Inner = { value: SyntaxNode };

    type Outer = { inner: Inner; label: string };
    ");
}

#[test]
fn emit_no_inline_synthetic() {
    let input = indoc! {r#"
        Container = { <Inner field> @inner }
    "#};
    let config = TypeScriptEmitConfig {
        inline_synthetic: false,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @r"
    interface Container {
      inner: InnerField;
    }
    ");
}

#[test]
fn emit_inline_synthetic() {
    let input = indoc! {r#"
        Container = { <Inner field> @inner }
    "#};
    insta::assert_snapshot!(emit(input), @r"
    interface Container {
      inner: InnerField;
    }
    ");
}

// --- Complex Scenarios ---

#[test]
fn emit_complex_program() {
    let input = indoc! {r#"
        FuncInfo = { #string @name #Node @body }
        Param = { #string @name #string @type_annotation }
        Params = Param*
        FuncDecl = { FuncInfo @info Params @params }
        ExprStmt = { #Node @expr }
        Stmt = [ Func: FuncDecl Expr: ExprStmt ]
        Program = { Stmt @statements }
    "#};
    insta::assert_snapshot!(emit(input), @r#"
    interface FuncInfo {
      name: string;
      body: SyntaxNode;
    }

    interface Param {
      name: string;
      type_annotation: string;
    }

    type Params = Param[];

    interface FuncDecl {
      info: FuncInfo;
      params: Param[];
    }

    interface ExprStmt {
      expr: SyntaxNode;
    }

    type Stmt =
      | { tag: "Func"; info: FuncInfo; params: Param[] }
      | { tag: "Expr"; expr: SyntaxNode };

    interface Program {
      statements: Stmt;
    }
    "#);
}

#[test]
fn emit_mixed_wrappers_and_structs() {
    let input = indoc! {r#"
        Leaf = { #string @text }
        Branch = { #Node @left #Node @right }
        Tree = [ Leaf: Leaf Branch: Branch ]
        Forest = Tree*
        MaybeForest = Forest?
    "#};
    insta::assert_snapshot!(emit(input), @r#"
    interface Leaf {
      text: string;
    }

    interface Branch {
      left: SyntaxNode;
      right: SyntaxNode;
    }

    type Tree =
      | { tag: "Leaf"; text: string }
      | { tag: "Branch"; left: SyntaxNode; right: SyntaxNode };

    type Forest = Tree[];

    type MaybeForest = Tree[] | null;
    "#);
}

#[test]
fn emit_all_config_options() {
    let input = indoc! {r#"
        MaybeNode = #Node?
        Item = { #Node @value MaybeNode @maybe }
        Items = Item*
    "#};
    let config = TypeScriptEmitConfig {
        optional_style: OptionalStyle::QuestionMark,
        export: true,
        readonly: true,
        inline_synthetic: true,
        node_type_name: "ASTNode".to_string(),
        use_type_alias: false,
        default_query_name: "QueryResult".to_string(),
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @r"
    export type MaybeNode = ASTNode;

    export interface Item {
      readonly value: ASTNode;
      readonly maybe?: ASTNode;
    }

    export type Items = Item[];
    ");
}

// --- Edge Cases ---

#[test]
fn emit_single_variant_union() {
    let input = indoc! {r#"
        OnlyVariant = { #Node @value }
        Single = [ Only: OnlyVariant ]
    "#};
    insta::assert_snapshot!(emit(input), @r#"
    interface OnlyVariant {
      value: SyntaxNode;
    }

    type Single =
      | { tag: "Only"; value: SyntaxNode };
    "#);
}

#[test]
fn emit_deeply_nested() {
    let input = indoc! {r#"
        A = { #Node @val }
        B = { A @a }
        C = { B @b }
        D = { C @c }
    "#};
    insta::assert_snapshot!(emit(input), @r"
    interface A {
      val: SyntaxNode;
    }

    interface B {
      a: A;
    }

    interface C {
      b: B;
    }

    interface D {
      c: C;
    }
    ");
}

#[test]
fn emit_union_in_list() {
    let input = indoc! {r#"
        A = { #Node @a }
        B = { #Node @b }
        Choice = [ A: A B: B ]
        Choices = Choice*
    "#};
    insta::assert_snapshot!(emit(input), @r#"
    interface A {
      a: SyntaxNode;
    }

    interface B {
      b: SyntaxNode;
    }

    type Choice =
      | { tag: "A"; a: SyntaxNode }
      | { tag: "B"; b: SyntaxNode };

    type Choices = Choice[];
    "#);
}

#[test]
fn emit_optional_in_struct_null_style() {
    let input = indoc! {r#"
        MaybeNode = #Node?
        Container = { MaybeNode @item #string @name }
    "#};
    insta::assert_snapshot!(emit(input), @r"
    type MaybeNode = SyntaxNode | null;

    interface Container {
      item: SyntaxNode | null;
      name: string;
    }
    ");
}

#[test]
fn emit_optional_in_struct_undefined_style() {
    let input = indoc! {r#"
        MaybeNode = #Node?
        Container = { MaybeNode @item #string @name }
    "#};
    let config = TypeScriptEmitConfig {
        optional_style: OptionalStyle::Undefined,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @r"
    type MaybeNode = SyntaxNode | undefined;

    interface Container {
      item: SyntaxNode | undefined;
      name: string;
    }
    ");
}

#[test]
fn emit_tagged_union_with_optional_field_question_mark() {
    let input = indoc! {r#"
        MaybeNode = #Node?
        VariantA = { MaybeNode @value }
        VariantB = { #Node @item }
        Choice = [ A: VariantA B: VariantB ]
    "#};
    let config = TypeScriptEmitConfig {
        optional_style: OptionalStyle::QuestionMark,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @r#"
    type MaybeNode = SyntaxNode;

    interface VariantA {
      value?: SyntaxNode;
    }

    interface VariantB {
      item: SyntaxNode;
    }

    type Choice =
      | { tag: "A"; value?: SyntaxNode }
      | { tag: "B"; item: SyntaxNode };
    "#);
}

#[test]
fn emit_struct_with_union_field() {
    let input = indoc! {r#"
        A = { #Node @a }
        B = { #Node @b }
        Choice = [ A: A B: B ]
        Container = { Choice @choice #string @name }
    "#};
    insta::assert_snapshot!(emit(input), @r#"
    interface A {
      a: SyntaxNode;
    }

    interface B {
      b: SyntaxNode;
    }

    type Choice =
      | { tag: "A"; a: SyntaxNode }
      | { tag: "B"; b: SyntaxNode };

    interface Container {
      choice: Choice;
      name: string;
    }
    "#);
}

#[test]
fn emit_struct_with_forward_ref() {
    let input = indoc! {r#"
        Container = { Later @item }
        Later = { #Node @value }
    "#};
    insta::assert_snapshot!(emit(input), @r"
    interface Later {
      value: SyntaxNode;
    }

    interface Container {
      item: Later;
    }
    ");
}

#[test]
fn emit_synthetic_type_no_inline() {
    let input = "<Foo bar> = { #Node @value }";
    let config = TypeScriptEmitConfig {
        inline_synthetic: false,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @r"
    interface FooBar {
      value: SyntaxNode;
    }
    ");
}

#[test]
fn emit_synthetic_type_with_inline() {
    let input = "<Foo bar> = { #Node @value }";
    let config = TypeScriptEmitConfig {
        inline_synthetic: true,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @"");
}

#[test]
fn emit_field_referencing_tagged_union() {
    let input = indoc! {r#"
        VarA = { #Node @x }
        VarB = { #Node @y }
        Choice = [ A: VarA B: VarB ]
        Container = { Choice @choice }
    "#};
    insta::assert_snapshot!(emit(input), @r#"
    interface VarA {
      x: SyntaxNode;
    }

    interface VarB {
      y: SyntaxNode;
    }

    type Choice =
      | { tag: "A"; x: SyntaxNode }
      | { tag: "B"; y: SyntaxNode };

    interface Container {
      choice: Choice;
    }
    "#);
}

#[test]
fn emit_field_referencing_unknown_type() {
    let input = "Container = { DoesNotExist @unknown }";
    insta::assert_snapshot!(emit(input), @r"
    interface Container {
      unknown: DoesNotExist;
    }
    ");
}

#[test]
fn emit_empty_interface_no_type_alias() {
    let input = "Empty = {}";
    let config = TypeScriptEmitConfig {
        use_type_alias: false,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @"interface Empty {}");
}

#[test]
fn emit_inline_synthetic_struct_with_optional_field() {
    let input = indoc! {r#"
        MaybeNode = #Node?
        <Inner nested> = { #Node @value MaybeNode @maybe }
        Container = { <Inner nested> @inner }
    "#};
    let config = TypeScriptEmitConfig {
        inline_synthetic: true,
        optional_style: OptionalStyle::QuestionMark,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @r"
    type MaybeNode = SyntaxNode;

    interface Container {
      inner: { value: SyntaxNode; maybe?: SyntaxNode };
    }
    ");
}

#[test]
fn emit_builtin_value_with_named_key() {
    let input = indoc! {r#"
        AliasNode = #Node
        AliasString = #string
        AliasUnit = ()
    "#};
    insta::assert_snapshot!(emit(input), @"");
}
