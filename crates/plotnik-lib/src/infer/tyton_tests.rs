use super::tyton::{emit, parse};
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
    Invalid = Invalid
    ");
}

#[test]
fn parse_struct_simple() {
    let input = "Foo = { #Node @name }";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("Foo") = Struct({"name": Node})
    "#);
}

#[test]
fn parse_struct_multiple_fields() {
    let input = "Func = { #string @name #Node @body #Node @params }";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
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
    Invalid = Invalid
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
    Invalid = Invalid
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
    Invalid = Invalid
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
    Invalid = Invalid
    Named("Single") = TaggedUnion({"Only": Named("OnlyVariant")})
    "#);
}

#[test]
fn parse_tagged_union_with_builtins() {
    let input = "Mixed = [ Text: #string Code: #Node Empty: () ]";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("Mixed") = TaggedUnion({"Text": String, "Code": Node, "Empty": Unit})
    "#);
}

#[test]
fn parse_optional() {
    let input = "MaybeNode = #Node?";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("MaybeNode") = Optional(Node)
    "#);
}

#[test]
fn parse_list() {
    let input = "Nodes = #Node*";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("Nodes") = List(Node)
    "#);
}

#[test]
fn parse_non_empty_list() {
    let input = "Nodes = #Node+";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
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
    Invalid = Invalid
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
    Invalid = Invalid
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
    Invalid = Invalid
    Named("Wrapper") = Optional(Synthetic { parent: Named("Foo"), name: "bar" })
    "#);
}

#[test]
fn parse_synthetic_key_multiple_segments() {
    let input = "Wrapper = <Foo bar baz>*";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("Wrapper") = List(Synthetic { parent: Synthetic { parent: Named("Foo"), name: "bar" }, name: "baz" })
    "#);
}

#[test]
fn parse_struct_with_synthetic() {
    let input = "Container = { <Inner field> @inner }";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("Container") = Struct({"inner": Synthetic { parent: Named("Inner"), name: "field" }})
    "#);
}

#[test]
fn parse_union_with_synthetic() {
    let input = "Choice = [ First: <Choice first> Second: <Choice second> ]";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("Choice") = TaggedUnion({"First": Synthetic { parent: Named("Choice"), name: "first" }, "Second": Synthetic { parent: Named("Choice"), name: "second" }})
    "#);
}

#[test]
fn parse_multiple_definitions() {
    let input = indoc! {r#"
        AssignStmt = { #Node @target #Node @value }
        CallStmt = { #Node @func #Node @args }
        Stmt = [ Assign: AssignStmt Call: CallStmt ]
        Stmts = Stmt*
    "#};
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("AssignStmt") = Struct({"target": Node, "value": Node})
    Named("CallStmt") = Struct({"func": Node, "args": Node})
    Named("Stmt") = TaggedUnion({"Assign": Named("AssignStmt"), "Call": Named("CallStmt")})
    Named("Stmts") = List(Named("Stmt"))
    "#);
}

#[test]
fn parse_complex_example() {
    let input = indoc! {r#"
        FuncInfo = { #string @name #Node @body }
        Param = { #string @name #string @type_annotation }
        Params = Param*
        FuncDecl = { FuncInfo @info Params @params }
        Stmt = [ Func: FuncDecl Expr: #Node ]
        MaybeStmt = Stmt?
        Program = { Stmt @statements }
    "#};
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
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
        AllBuiltins = { #Node @node #string @str () @unit }
        OptNode = #Node?
        ListStr = #string*
        NonEmptyUnit = ()+
    "#};
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("AllBuiltins") = Struct({"node": Node, "str": String, "unit": Unit})
    Named("OptNode") = Optional(Node)
    Named("ListStr") = List(String)
    Named("NonEmptyUnit") = NonEmptyList(Unit)
    "#);
}

#[test]
fn parse_invalid_builtin() {
    let input = "HasInvalid = { #Invalid @bad }";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("HasInvalid") = Struct({"bad": Invalid})
    "#);
}

#[test]
fn parse_invalid_wrapper() {
    let input = "MaybeInvalid = #Invalid?";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("MaybeInvalid") = Optional(Invalid)
    "#);
}

#[test]
fn error_missing_eq() {
    let input = "Foo { #Node @x }";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected Eq, got LBrace at 4..5");
}

#[test]
fn error_missing_at() {
    let input = "Foo = { #Node name }";
    insta::assert_snapshot!(dump_table(input), @r#"ERROR: expected At, got LowerIdent("name") at 14..18"#);
}

#[test]
fn error_missing_colon_in_union() {
    let input = "Foo = [ A B ]";
    insta::assert_snapshot!(dump_table(input), @r#"ERROR: expected Colon, got UpperIdent("B") at 10..11"#);
}

#[test]
fn error_empty_synthetic() {
    let input = "Foo = <>?";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected parent key (uppercase name, #DefaultQuery, or <...>) at 7..8");
}

#[test]
fn error_unclosed_brace() {
    let input = "Foo = { #Node @x";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected type key at 16..16");
}

#[test]
fn error_unclosed_bracket() {
    let input = "Foo = [ A: B";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected variant tag (uppercase) at 12..12");
}

#[test]
fn error_lowercase_type_name() {
    let input = "foo = { #Node @x }";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected type name (uppercase) or synthetic key at 0..3");
}

#[test]
fn error_uppercase_field_name() {
    let input = "Foo = { #Node @Name }";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected field name (lowercase) at 15..19");
}

#[test]
fn parse_bare_builtin_alias_node() {
    let input = "AliasNode = #Node";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("AliasNode") = Node
    "#);
}

#[test]
fn parse_bare_builtin_alias_string() {
    let input = "AliasString = #string";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("AliasString") = String
    "#);
}

#[test]
fn parse_bare_builtin_alias_unit() {
    let input = "AliasUnit = ()";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("AliasUnit") = Unit
    "#);
}

#[test]
fn parse_bare_builtin_alias_invalid() {
    let input = "AliasInvalid = #Invalid";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("AliasInvalid") = Invalid
    "#);
}

#[test]
fn parse_synthetic_definition_struct() {
    let input = "<Foo bar> = { #Node @value }";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Synthetic { parent: Named("Foo"), name: "bar" } = Struct({"value": Node})
    "#);
}

#[test]
fn parse_synthetic_definition_union() {
    let input = "<Choice first> = [ A: #Node B: #string ]";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Synthetic { parent: Named("Choice"), name: "first" } = TaggedUnion({"A": Node, "B": String})
    "#);
}

#[test]
fn parse_synthetic_definition_wrapper() {
    let input = "<Inner nested> = #Node?";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Synthetic { parent: Named("Inner"), name: "nested" } = Optional(Node)
    "#);
}

#[test]
fn error_invalid_char() {
    let input = "Foo = { #Node @x $ }";
    insta::assert_snapshot!(dump_table(input), @r#"ERROR: unexpected character: "$" at 17..18"#);
}

#[test]
fn error_eof_in_struct() {
    let input = "Foo = { #Node @x";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected type key at 16..16");
}

#[test]
fn error_eof_expecting_colon() {
    let input = "Foo = [ A";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected Colon, got EOF at 9..9");
}

#[test]
fn error_invalid_token_in_synthetic() {
    let input = "Foo = <A @>?";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected path segment (lowercase) or '>' at 9..10");
}

#[test]
fn error_invalid_type_value() {
    let input = "Foo = @bar";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected type value at 6..7");
}

#[test]
fn error_unprefixed_node() {
    let input = "Foo = { Node @x }";
    insta::assert_snapshot!(dump_table(input), @r#"
    Node = Node
    String = String
    Unit = Unit
    Invalid = Invalid
    Named("Foo") = Struct({"x": Named("Node")})
    "#);
}

#[test]
fn error_unprefixed_string() {
    let input = "Foo = string";
    insta::assert_snapshot!(dump_table(input), @"ERROR: expected type value at 6..12");
}

#[test]
fn emit_empty() {
    let table = parse("").unwrap();
    insta::assert_snapshot!(emit(&table), @"");
}

#[test]
fn emit_struct_simple() {
    let table = parse("Foo = { #Node @name }").unwrap();
    insta::assert_snapshot!(emit(&table), @"Foo = { #Node @name }");
}

#[test]
fn emit_struct_multiple_fields() {
    let table = parse("Func = { #string @name #Node @body #Node @params }").unwrap();
    insta::assert_snapshot!(emit(&table), @"Func = { #string @name #Node @body #Node @params }");
}

#[test]
fn emit_struct_empty() {
    let table = parse("Empty = {}").unwrap();
    insta::assert_snapshot!(emit(&table), @"Empty = {  }");
}

#[test]
fn emit_tagged_union() {
    let table = parse("Stmt = [ Assign: AssignStmt Call: CallStmt ]").unwrap();
    insta::assert_snapshot!(emit(&table), @"Stmt = [ Assign: AssignStmt Call: CallStmt ]");
}

#[test]
fn emit_optional() {
    let table = parse("MaybeNode = #Node?").unwrap();
    insta::assert_snapshot!(emit(&table), @"MaybeNode = #Node?");
}

#[test]
fn emit_list() {
    let table = parse("Nodes = #Node*").unwrap();
    insta::assert_snapshot!(emit(&table), @"Nodes = #Node*");
}

#[test]
fn emit_non_empty_list() {
    let table = parse("Nodes = #Node+").unwrap();
    insta::assert_snapshot!(emit(&table), @"Nodes = #Node+");
}

#[test]
fn emit_synthetic_key() {
    let table = parse("<Foo bar> = { #Node @value }").unwrap();
    insta::assert_snapshot!(emit(&table), @"<Foo bar> = { #Node @value }");
}

#[test]
fn emit_synthetic_in_wrapper() {
    let table = parse("Wrapper = <Foo bar>?").unwrap();
    insta::assert_snapshot!(emit(&table), @"Wrapper = <Foo bar>?");
}

#[test]
fn emit_bare_builtins() {
    let input = indoc! {r#"
        AliasNode = #Node
        AliasString = #string
        AliasUnit = ()
    "#};
    let table = parse(input).unwrap();
    insta::assert_snapshot!(emit(&table), @r"
    AliasNode = #Node
    AliasString = #string
    AliasUnit = ()
    ");
}

#[test]
fn emit_multiple_definitions() {
    let input = indoc! {r#"
        AssignStmt = { #Node @target #Node @value }
        CallStmt = { #Node @func #Node @args }
        Stmt = [ Assign: AssignStmt Call: CallStmt ]
        Stmts = Stmt*
    "#};
    let table = parse(input).unwrap();
    insta::assert_snapshot!(emit(&table), @r"
    AssignStmt = { #Node @target #Node @value }
    CallStmt = { #Node @func #Node @args }
    Stmt = [ Assign: AssignStmt Call: CallStmt ]
    Stmts = Stmt*
    ");
}

#[test]
fn emit_roundtrip() {
    let input = indoc! {r#"
        FuncInfo = { #string @name #Node @body }
        Param = { #string @name #string @type_annotation }
        Params = Param*
        FuncDecl = { FuncInfo @info Params @params }
        Stmt = [ Func: FuncDecl Expr: #Node ]
        MaybeStmt = Stmt?
    "#};

    let table1 = parse(input).unwrap();
    let emitted = emit(&table1);
    let table2 = parse(&emitted).unwrap();

    assert_eq!(table1.types, table2.types);
}
