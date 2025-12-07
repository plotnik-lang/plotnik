use super::rust::{Indirection, RustEmitConfig, emit_rust};
use crate::infer::tyton::parse;
use indoc::indoc;

fn emit(input: &str) -> String {
    let table = parse(input).expect("tyton parse failed");
    emit_rust(&table, &RustEmitConfig::default())
}

fn emit_with_config(input: &str, config: &RustEmitConfig) -> String {
    let table = parse(input).expect("tyton parse failed");
    emit_rust(&table, config)
}

fn emit_cyclic(input: &str, cyclic_types: &[&str]) -> String {
    let mut table = parse(input).expect("tyton parse failed");
    for name in cyclic_types {
        table.mark_cyclic(crate::infer::TypeKey::Named(name));
    }
    emit_rust(&table, &RustEmitConfig::default())
}

// --- Simple Structs ---

#[test]
fn emit_struct_single_field() {
    let input = "Foo = { #Node @value }";
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub struct Foo {
        pub value: Node,
    }
    ");
}

#[test]
fn emit_struct_multiple_fields() {
    let input = "Func = { #string @name #Node @body #Node @params }";
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub struct Func {
        pub name: String,
        pub body: Node,
        pub params: Node,
    }
    ");
}

#[test]
fn emit_struct_empty() {
    let input = "Empty = {}";
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub struct Empty;
    ");
}

#[test]
fn emit_struct_with_unit_field() {
    let input = "Wrapper = { () @marker }";
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub struct Wrapper {
        pub marker: (),
    }
    ");
}

#[test]
fn emit_struct_nested_refs() {
    let input = indoc! {r#"
        Inner = { #Node @value }
        Outer = { Inner @inner #string @label }
    "#};
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub struct Inner {
        pub value: Node,
    }

    #[derive(Debug, Clone)]
    pub struct Outer {
        pub inner: Inner,
        pub label: String,
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
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub struct AssignStmt {
        pub target: Node,
        pub value: Node,
    }

    #[derive(Debug, Clone)]
    pub struct CallStmt {
        pub func: Node,
    }

    #[derive(Debug, Clone)]
    pub enum Stmt {
        Assign {
            target: Node,
            value: Node,
        },
        Call {
            func: Node,
        },
    }
    ");
}

#[test]
fn emit_tagged_union_with_empty_variant() {
    let input = indoc! {r#"
        ValueVariant = { #Node @value }
        Expr = [ Some: ValueVariant None: () ]
    "#};
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub struct ValueVariant {
        pub value: Node,
    }

    #[derive(Debug, Clone)]
    pub enum Expr {
        Some {
            value: Node,
        },
        None,
    }
    ");
}

#[test]
fn emit_tagged_union_all_empty() {
    let input = "Token = [ Comma: () Dot: () Semi: () ]";
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub enum Token {
        Comma,
        Dot,
        Semi,
    }
    ");
}

#[test]
fn emit_tagged_union_with_builtins() {
    let input = "Value = [ Text: #string Code: #Node Empty: () ]";
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub enum Value {
        Text,
        Code,
        Empty,
    }
    ");
}

// --- Wrapper Types ---

#[test]
fn emit_optional() {
    let input = "MaybeNode = #Node?";
    insta::assert_snapshot!(emit(input), @"pub type MaybeNode = Option<Node>;");
}

#[test]
fn emit_list() {
    let input = "Nodes = #Node*";
    insta::assert_snapshot!(emit(input), @"pub type Nodes = Vec<Node>;");
}

#[test]
fn emit_non_empty_list() {
    let input = "Nodes = #Node+";
    insta::assert_snapshot!(emit(input), @"pub type Nodes = Vec<Node>;");
}

#[test]
fn emit_optional_named() {
    let input = indoc! {r#"
        Stmt = { #Node @value }
        MaybeStmt = Stmt?
    "#};
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub struct Stmt {
        pub value: Node,
    }

    pub type MaybeStmt = Option<Stmt>;
    ");
}

#[test]
fn emit_list_named() {
    let input = indoc! {r#"
        Stmt = { #Node @value }
        Stmts = Stmt*
    "#};
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub struct Stmt {
        pub value: Node,
    }

    pub type Stmts = Vec<Stmt>;
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
    #[derive(Debug, Clone)]
    pub struct Item {
        pub value: Node,
    }

    pub type Items = Vec<Item>;

    pub type MaybeItems = Option<Vec<Item>>;
    ");
}

// --- Cyclic Types ---

#[test]
fn emit_cyclic_box() {
    let input = indoc! {r#"
        TreeNode = { #Node @value TreeNode @left TreeNode @right }
    "#};
    insta::assert_snapshot!(emit_cyclic(input, &["TreeNode"]), @r"
    #[derive(Debug, Clone)]
    pub struct TreeNode {
        pub value: Node,
        pub left: Box<TreeNode>,
        pub right: Box<TreeNode>,
    }
    ");
}

#[test]
fn emit_cyclic_rc() {
    let input = "TreeNode = { #Node @value TreeNode @child }";
    let config = RustEmitConfig {
        indirection: Indirection::Rc,
        ..Default::default()
    };
    let mut table = parse(input).expect("tyton parse failed");
    table.mark_cyclic(crate::infer::TypeKey::Named("TreeNode"));
    insta::assert_snapshot!(emit_rust(&table, &config), @r"
    #[derive(Debug, Clone)]
    pub struct TreeNode {
        pub value: Node,
        pub child: Rc<TreeNode>,
    }
    ");
}

#[test]
fn emit_cyclic_arc() {
    let input = "TreeNode = { #Node @value TreeNode @child }";
    let config = RustEmitConfig {
        indirection: Indirection::Arc,
        ..Default::default()
    };
    let mut table = parse(input).expect("tyton parse failed");
    table.mark_cyclic(crate::infer::TypeKey::Named("TreeNode"));
    insta::assert_snapshot!(emit_rust(&table, &config), @r"
    #[derive(Debug, Clone)]
    pub struct TreeNode {
        pub value: Node,
        pub child: Arc<TreeNode>,
    }
    ");
}

// --- Config Variations ---

#[test]
fn emit_no_derives() {
    let input = "Foo = { #Node @value }";
    let config = RustEmitConfig {
        derive_debug: false,
        derive_clone: false,
        derive_partial_eq: false,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @r"
    pub struct Foo {
        pub value: Node,
    }
    ");
}

#[test]
fn emit_debug_only() {
    let input = "Foo = { #Node @value }";
    let config = RustEmitConfig {
        derive_debug: true,
        derive_clone: false,
        derive_partial_eq: false,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @r"
    #[derive(Debug)]
    pub struct Foo {
        pub value: Node,
    }
    ");
}

#[test]
fn emit_all_derives() {
    let input = "Foo = { #Node @value }";
    let config = RustEmitConfig {
        derive_debug: true,
        derive_clone: true,
        derive_partial_eq: true,
        ..Default::default()
    };
    insta::assert_snapshot!(emit_with_config(input, &config), @r"
    #[derive(Debug, Clone, PartialEq)]
    pub struct Foo {
        pub value: Node,
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
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub struct FuncInfo {
        pub name: String,
        pub body: Node,
    }

    #[derive(Debug, Clone)]
    pub struct Param {
        pub name: String,
        pub type_annotation: String,
    }

    pub type Params = Vec<Param>;

    #[derive(Debug, Clone)]
    pub struct FuncDecl {
        pub info: FuncInfo,
        pub params: Vec<Param>,
    }

    #[derive(Debug, Clone)]
    pub struct ExprStmt {
        pub expr: Node,
    }

    #[derive(Debug, Clone)]
    pub enum Stmt {
        Func {
            info: FuncInfo,
            params: Vec<Param>,
        },
        Expr {
            expr: Node,
        },
    }

    #[derive(Debug, Clone)]
    pub struct Program {
        pub statements: Stmt,
    }
    ");
}

#[test]
fn emit_synthetic_keys() {
    let input = indoc! {r#"
        Container = { <Inner field> @inner }
        InnerWrapper = <Inner field>?
    "#};
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub struct Container {
        pub inner: InnerField,
    }

    pub type InnerWrapper = Option<InnerField>;
    ");
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
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub struct Leaf {
        pub text: String,
    }

    #[derive(Debug, Clone)]
    pub struct Branch {
        pub left: Node,
        pub right: Node,
    }

    #[derive(Debug, Clone)]
    pub enum Tree {
        Leaf {
            text: String,
        },
        Branch {
            left: Node,
            right: Node,
        },
    }

    pub type Forest = Vec<Tree>;

    pub type MaybeForest = Option<Vec<Tree>>;
    ");
}

// --- Edge Cases ---

#[test]
fn emit_single_variant_union() {
    let input = indoc! {r#"
        OnlyVariant = { #Node @value }
        Single = [ Only: OnlyVariant ]
    "#};
    insta::assert_snapshot!(emit(input), @r"
    #[derive(Debug, Clone)]
    pub struct OnlyVariant {
        pub value: Node,
    }

    #[derive(Debug, Clone)]
    pub enum Single {
        Only {
            value: Node,
        },
    }
    ");
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
    #[derive(Debug, Clone)]
    pub struct A {
        pub val: Node,
    }

    #[derive(Debug, Clone)]
    pub struct B {
        pub a: A,
    }

    #[derive(Debug, Clone)]
    pub struct C {
        pub b: B,
    }

    #[derive(Debug, Clone)]
    pub struct D {
        pub c: C,
    }
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
    #[derive(Debug, Clone)]
    pub struct Item {
        pub value: Node,
    }

    pub type MaybeItem = Option<Item>;

    pub type Items = Vec<Option<Item>>;
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
