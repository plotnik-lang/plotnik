use super::tyton::parse;
use indoc::indoc;

fn dump_table(input: &str) -> String {
    match parse(input) {
        Ok(table) => {
            let mut out = String::new();
            for (key, value) in table.iter() {
                out.push_str(&format!("{:?} = {:?}\n", key, value));
            }
            out
        }
        Err(e) => format!("ERROR: {}", e),
    }
}

#[test]
fn parse_empty() {
    insta::assert_snapshot!(dump_table(""), @r"
    Node = Node
    String = String
    Unit = Unit
    ");
}

#[test]
fn parse_struct_simple() {
    let input = "Foo = { Node @name }";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("Foo") = Struct({"name": Node})
    "#);
}

#[test]
fn parse_struct_multiple_fields() {
    let input = "Func = { string @name Node @body Node @params }";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("Func") = Struct({"name": String, "body": Node, "params": Node})
    "#);
}

#[test]
fn parse_struct_empty() {
    let input = "Empty = {}";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("Empty") = Struct({})
    "#);
}

#[test]
fn parse_struct_with_unit() {
    let input = "Wrapper = { () @unit }";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("Wrapper") = Struct({"unit": Unit})
    "#);
}

#[test]
fn parse_tagged_union() {
    let input = "Stmt = [ Assign: AssignStmt Call: CallStmt ]";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("Stmt") = TaggedUnion({"Assign": Named("AssignStmt"), "Call": Named("CallStmt")})
    "#);
}

#[test]
fn parse_tagged_union_single() {
    let input = "Single = [ Only: OnlyVariant ]";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("Single") = TaggedUnion({"Only": Named("OnlyVariant")})
    "#);
}

#[test]
fn parse_tagged_union_with_builtins() {
    let input = "Mixed = [ Text: string Code: Node Empty: () ]";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("Mixed") = TaggedUnion({"Text": String, "Code": Node, "Empty": Unit})
    "#);
}

#[test]
fn parse_optional() {
    let input = "MaybeNode = Node?";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("MaybeNode") = Optional(Node)
    "#);
}

#[test]
fn parse_list() {
    let input = "Nodes = Node*";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("Nodes") = List(Node)
    "#);
}

#[test]
fn parse_non_empty_list() {
    let input = "Nodes = Node+";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("Nodes") = NonEmptyList(Node)
    "#);
}

#[test]
fn parse_optional_named() {
    let input = "MaybeStmt = Stmt?";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("MaybeStmt") = Optional(Named("Stmt"))
    "#);
}

#[test]
fn parse_list_named() {
    let input = "Stmts = Stmt*";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("Stmts") = List(Named("Stmt"))
    "#);
}

#[test]
fn parse_synthetic_key_simple() {
    let input = "Wrapper = <Foo bar>?";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("Wrapper") = Optional(Synthetic(["Foo", "bar"]))
    "#);
}

#[test]
fn parse_synthetic_key_multiple_segments() {
    let input = "Wrapper = <Foo bar baz>*";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("Wrapper") = List(Synthetic(["Foo", "bar", "baz"]))
    "#);
}

#[test]
fn parse_struct_with_synthetic() {
    let input = "Container = { <Inner field> @inner }";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("Container") = Struct({"inner": Synthetic(["Inner", "field"])})
    "#);
}

#[test]
fn parse_union_with_synthetic() {
    let input = "Choice = [ First: <Choice first> Second: <Choice second> ]";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("Choice") = TaggedUnion({"First": Synthetic(["Choice", "first"]), "Second": Synthetic(["Choice", "second"])})
    "#);
}

#[test]
fn parse_multiple_definitions() {
    let input = indoc! {r#"
        AssignStmt = { Node @target Node @value }
        CallStmt = { Node @func Node @args }
        Stmt = [ Assign: AssignStmt Call: CallStmt ]
        Stmts = Stmt*
    "#};
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("AssignStmt") = Struct({"target": Node, "value": Node})
    Named("CallStmt") = Struct({"func": Node, "args": Node})
    Named("Stmt") = TaggedUnion({"Assign": Named("AssignStmt"), "Call": Named("CallStmt")})
    Named("Stmts") = List(Named("Stmt"))
    "#);
}

#[test]
fn parse_complex_example() {
    let input = indoc! {r#"
        FuncInfo = { string @name Node @body }
        Param = { string @name string @type_annotation }
        Params = Param*
        FuncDecl = { FuncInfo @info Params @params }
        Stmt = [ Func: FuncDecl Expr: Node ]
        MaybeStmt = Stmt?
        Program = { Stmt @statements }
    "#};
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("FuncInfo") = Struct({"name": String, "body": Node})
    Named("Param") = Struct({"name": String, "type_annotation": String})
    Named("Params") = List(Named("Param"))
    Named("FuncDecl") = Struct({"info": Named("FuncInfo"), "params": Named("Params")})
    Named("Stmt") = TaggedUnion({"Func": Named("FuncDecl"), "Expr": Node})
    Named("MaybeStmt") = Optional(Named("Stmt"))
    Named("Program") = Struct({"statements": Named("Stmt")})
    "#);
}

#[test]
fn parse_all_builtins() {
    let input = indoc! {r#"
        AllBuiltins = { Node @node string @str () @unit }
        OptNode = Node?
        ListStr = string*
        NonEmptyUnit = ()+
    "#};
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Named("AllBuiltins") = Struct({"node": Node, "str": String, "unit": Unit})
    Named("OptNode") = Optional(Node)
    Named("ListStr") = List(String)
    Named("NonEmptyUnit") = NonEmptyList(Unit)
    "#);
}

#[test]
fn error_missing_eq() {
    let input = "Foo { Node @x }";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected Eq, got LBrace at 4..5");
}

#[test]
fn error_missing_at() {
    let input = "Foo = { Node name }";
    insta::assert_snapshot!(dump_table(input), @r#"ERROR: expected At, got LowerIdent("name") at 13..17"#);
}

#[test]
fn error_missing_colon_in_union() {
    let input = "Foo = [ A B ]";
    insta::assert_snapshot!(dump_table(input), @r#"ERROR: expected Colon, got UpperIdent("B") at 10..11"#);
}

#[test]
fn error_empty_synthetic() {
    let input = "Foo = <>?";
    insta::assert_snapshot!(dump_table(input), @"ERROR: synthetic key cannot be empty at 8..9");
}

#[test]
fn error_unclosed_brace() {
    let input = "Foo = { Node @x";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected type key at 15..15");
}

#[test]
fn error_unclosed_bracket() {
    let input = "Foo = [ A: B";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected variant tag (uppercase) at 12..12");
}

#[test]
fn error_lowercase_type_name() {
    let input = "foo = { Node @x }";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected type name (uppercase) at 0..3");
}

#[test]
fn error_uppercase_field_name() {
    let input = "Foo = { Node @Name }";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected field name (lowercase) at 14..18");
}

#[test]
fn error_missing_quantifier() {
    let input = "Foo = Node";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected quantifier (?, *, +) after type key at 10..10");
}

#[test]
fn error_invalid_char() {
    let input = "Foo = { Node @x $ }";
    insta::assert_snapshot!(dump_table(input), @r#"ERROR: unexpected character: "$" at 16..17"#);
}
