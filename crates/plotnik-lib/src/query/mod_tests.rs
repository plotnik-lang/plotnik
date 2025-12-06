use super::*;

#[test]
fn valid_query() {
    let q = Query::try_from("Expr = (expression)").unwrap();
    assert!(q.is_valid());
}

#[test]
fn parse_error() {
    let q = Query::try_from("(unclosed").unwrap();
    assert!(!q.is_valid());
    insta::assert_snapshot!(q.dump_diagnostics(), @r"
    error: missing closing `)`; expected `)`
      |
    1 | (unclosed
      | -^^^^^^^^
      | |
      | tree started here
    ");
}

#[test]
fn resolution_error() {
    let q = Query::try_from("(call (Undefined))").unwrap();
    assert!(!q.is_valid());
    insta::assert_snapshot!(q.dump_diagnostics(), @r"
    error: `Undefined` is not defined
      |
    1 | (call (Undefined))
      |        ^^^^^^^^^
    ");
}

#[test]
fn combined_errors() {
    let q = Query::try_from("(call (Undefined) extra)").unwrap();
    assert!(!q.is_valid());
    insta::assert_snapshot!(q.dump_diagnostics(), @r"
    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | (call (Undefined) extra)
      |                   ^^^^^
    ");
}
