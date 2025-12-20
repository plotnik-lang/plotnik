use crate::Query;
use indoc::indoc;

#[test]
fn single_definition() {
    let input = "Expr = (expression)";
    let res = Query::expect_valid_symbols(input);

    insta::assert_snapshot!(res, @"Expr");
}

#[test]
fn multiple_definitions() {
    let input = indoc! {r#"
    Expr = (expression)
    Stmt = (statement)
    Decl = (declaration)
    "#};

    let res = Query::expect_valid_symbols(input);

    insta::assert_snapshot!(res, @r"
    Expr
    Stmt
    Decl
    ");
}

#[test]
fn valid_reference() {
    let input = indoc! {r#"
    Expr = (expression)
    Call = (call_expression function: (Expr))
    "#};

    let res = Query::expect_valid_symbols(input);

    insta::assert_snapshot!(res, @r"
    Expr
    Call
      Expr
    ");
}

#[test]
fn undefined_reference() {
    let input = "Call = (call_expression function: (Undefined))";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `Undefined` is not defined
      |
    1 | Call = (call_expression function: (Undefined))
      |                                    ^^^^^^^^^
    ");
}

#[test]
fn self_reference() {
    let input = "Expr = [(identifier) (call (Expr))]";

    let res = Query::expect_valid_symbols(input);

    insta::assert_snapshot!(res, @r"
    Expr
      Expr (cycle)
    ");
}

#[test]
fn mutual_recursion() {
    let input = indoc! {r#"
    A = (foo (B))
    B = (bar (A))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | A = (foo (B))
      |           - references B (completing cycle)
    2 | B = (bar (A))
      | -         ^
      | |         |
      | |         references A
      | B is defined here
    ");
}

#[test]
fn duplicate_definition() {
    let input = indoc! {r#"
    Expr = (expression)
    Expr = (other)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `Expr` is already defined
      |
    2 | Expr = (other)
      | ^^^^
    ");
}

#[test]
fn reference_in_alternation() {
    let input = indoc! {r#"
    Expr = (expression)
    Value = [(Expr) (literal)]
    "#};

    let res = Query::expect_valid_symbols(input);

    insta::assert_snapshot!(res, @r"
    Expr
    Value
      Expr
    ");
}

#[test]
fn reference_in_sequence() {
    let input = indoc! {r#"
    Expr = (expression)
    Pair = {(Expr) (Expr)}
    "#};

    let res = Query::expect_valid_symbols(input);

    insta::assert_snapshot!(res, @r"
    Expr
    Pair
      Expr
    ");
}

#[test]
fn reference_in_quantifier() {
    let input = indoc! {r#"
    Expr = (expression)
    List = (Expr)*
    "#};

    let res = Query::expect_valid_symbols(input);

    insta::assert_snapshot!(res, @r"
    Expr
    List
      Expr
    ");
}

#[test]
fn reference_in_capture() {
    let input = indoc! {r#"
    Expr = (expression)
    Named = (Expr) @e
    "#};

    let res = Query::expect_valid_symbols(input);

    insta::assert_snapshot!(res, @r"
    Expr
    Named
      Expr
    ");
}

#[test]
fn entry_point_reference() {
    let input = indoc! {r#"
    Expr = (expression)
    Q = (call function: (Expr))
    "#};

    let res = Query::expect_valid_symbols(input);

    insta::assert_snapshot!(res, @r"
    Expr
    Q
      Expr
    ");
}

#[test]
fn entry_point_undefined_reference() {
    let input = "Q = (call function: (Unknown))";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `Unknown` is not defined
      |
    1 | Q = (call function: (Unknown))
      |                      ^^^^^^^
    ");
}

#[test]
fn no_definitions() {
    let input = "Q = (identifier)";
    let res = Query::expect_valid_symbols(input);

    insta::assert_snapshot!(res, @"Q");
}

#[test]
fn nested_references() {
    let input = indoc! {r#"
    A = (a)
    B = (b (A))
    C = (c (B))
    D = (d (C) (A))
    "#};

    let res = Query::expect_valid_symbols(input);

    insta::assert_snapshot!(res, @r"
    A
    B
      A
    C
      B
        A
    D
      A
      C
        B
          A
    ");
}

#[test]
fn multiple_undefined() {
    let input = "Q = (foo (X) (Y) (Z))";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `X` is not defined
      |
    1 | Q = (foo (X) (Y) (Z))
      |           ^

    error: `Y` is not defined
      |
    1 | Q = (foo (X) (Y) (Z))
      |               ^

    error: `Z` is not defined
      |
    1 | Q = (foo (X) (Y) (Z))
      |                   ^
    ");
}

#[test]
fn reference_inside_tree_child() {
    let input = indoc! {r#"
        A = (a)
        B = (b (A))
    "#};

    let res = Query::expect_valid_symbols(input);

    insta::assert_snapshot!(res, @r"
    A
    B
      A
    ");
}

#[test]
fn reference_inside_capture() {
    let input = indoc! {r#"
        A = (a)
        B = (A)@x
    "#};

    let res = Query::expect_valid_symbols(input);

    insta::assert_snapshot!(res, @r"
    A
    B
      A
    ");
}
