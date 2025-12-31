//! Tests for bytecode dump functionality.

use crate::Query;
use indoc::indoc;

#[test]
fn dump_minimal() {
    let input = "Test = (identifier) @id";

    let res = Query::expect_valid_linked_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = true

    [strings]
    S00 "Beauty will save the world"
    S01 "id"
    S02 "Test"
    S03 "identifier"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 1)  ; { id }

    [types.members]
    M0 = (S01, T01)  ; id: Node

    [types.names]
    N0 = (S02, T03)  ; Test

    [entry]
    Test = 01 :: T03

    [code]
      00   𝜀                                    ◼

    Test:
      01   𝜀                                    02
      02  *↓   (identifier)                     03
      03   𝜀   [Node Set(M0)]                   ◼
    "#);
}

#[test]
fn dump_multiple_entrypoints() {
    let input = indoc! {r#"
        Expression = [(identifier) @name (number) @value]
        Root = (function_declaration name: (identifier) @name)
    "#};

    let res = Query::expect_valid_linked_bytecode(input);

    // Verify key sections exist
    assert!(res.contains("[header]"));
    assert!(res.contains("[strings]"));
    assert!(res.contains("[types.defs]"));
    assert!(res.contains("[types.members]"));
    assert!(res.contains("[types.names]"));
    assert!(res.contains("[entry]"));
    assert!(res.contains("[code]"));

    // Verify both entrypoints appear
    assert!(res.contains("Expression"));
    assert!(res.contains("Root"));

    // Verify code section has entrypoint labels
    assert!(res.contains("Expression:"));
    assert!(res.contains("Root:"));
}

#[test]
fn dump_with_field_constraints() {
    let input = indoc! {r#"
        Test = (binary_expression
            left: (_) @left
            right: (_) @right)
    "#};

    let res = Query::expect_valid_linked_bytecode(input);

    // Should have field references in code section
    assert!(res.contains("left:"));
    assert!(res.contains("right:"));
}

#[test]
fn dump_with_quantifier() {
    let input = "Test = (identifier)* @items";

    let res = Query::expect_valid_linked_bytecode(input);

    // Should have array type
    assert!(res.contains("Array") || res.contains("[]"));
}

#[test]
fn dump_with_alternation() {
    let input = "Test = [(identifier) @id (string) @str]";

    let res = Query::expect_valid_linked_bytecode(input);

    // Should have code section with branching
    assert!(res.contains("[code]"));
}

#[test]
fn dump_comprehensive() {
    // A query that exercises most features:
    // - Multiple definitions (entrypoints)
    // - Field constraints (node_fields)
    // - Multiple node types (node_types)
    // - Captures with types (type_defs, type_members)
    // - Alternation (branching in code)
    let input = indoc! {r#"
        Ident = (identifier) @name :: string
        Expression = [
            Literal: (number) @value
            Variable: (identifier) @name
        ]
        Assignment = (assignment_expression
            left: (identifier) @target
            right: (Expression) @value)
    "#};

    let res = Query::expect_valid_linked_bytecode(input);

    insta::assert_snapshot!(res);
}
