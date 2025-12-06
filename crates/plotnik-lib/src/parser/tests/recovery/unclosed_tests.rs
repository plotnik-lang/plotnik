use crate::Query;
use indoc::indoc;

#[test]
fn missing_paren() {
    let input = indoc! {r#"
    (identifier
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed tree: unclosed tree; expected ')'
      |
    1 | (identifier
      | -          ^
      | |
      | tree started here
    ");
}

#[test]
fn missing_bracket() {
    let input = indoc! {r#"
    [(identifier) (string)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed alternation: unclosed alternation; expected ']'
      |
    1 | [(identifier) (string)
      | -                     ^
      | |
      | alternation started here
    ");
}

#[test]
fn missing_brace() {
    let input = indoc! {r#"
    {(a) (b)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed sequence: unclosed sequence; expected '}'
      |
    1 | {(a) (b)
      | -       ^
      | |
      | sequence started here
    ");
}

#[test]
fn nested_unclosed() {
    let input = indoc! {r#"
    (a (b (c)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed tree: unclosed tree; expected ')'
      |
    1 | (a (b (c)
      |    -     ^
      |    |
      |    tree started here
    ");
}

#[test]
fn deeply_nested_unclosed() {
    let input = indoc! {r#"
    (a (b (c (d
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed tree: unclosed tree; expected ')'
      |
    1 | (a (b (c (d
      |          - ^
      |          |
      |          tree started here
    ");
}

#[test]
fn unclosed_alternation_nested() {
    let input = indoc! {r#"
    [(a) (b
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed tree: unclosed tree; expected ')'
      |
    1 | [(a) (b
      |      - ^
      |      |
      |      tree started here
    ");
}

#[test]
fn empty_parens() {
    let input = indoc! {r#"
    ()
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: empty tree expression: empty tree expression
      |
    1 | ()
      |  ^
    ");
}

#[test]
fn unclosed_tree_shows_open_location() {
    let input = indoc! {r#"
    (call
        (identifier)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed tree: unclosed tree; expected ')'
      |
    1 | (call
      | - tree started here
    2 |     (identifier)
      |                 ^
    ");
}

#[test]
fn unclosed_alternation_shows_open_location() {
    let input = indoc! {r#"
    [
        (a)
        (b)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed alternation: unclosed alternation; expected ']'
      |
    1 | [
      | - alternation started here
    2 |     (a)
    3 |     (b)
      |        ^
    ");
}

#[test]
fn unclosed_sequence_shows_open_location() {
    let input = indoc! {r#"
    {
        (a)
        (b)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed sequence: unclosed sequence; expected '}'
      |
    1 | {
      | - sequence started here
    2 |     (a)
    3 |     (b)
      |        ^
    ");
}

#[test]
fn unclosed_double_quote_string() {
    let input = r#"(call "foo)"#;

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token: unexpected token; expected a child expression or closing delimiter
      |
    1 | (call "foo)
      |       ^^^^^
    error: unclosed tree: unclosed tree; expected ')'
      |
    1 | (call "foo)
      | -          ^
      | |
      | tree started here
    "#);
}

#[test]
fn unclosed_single_quote_string() {
    let input = "(call 'foo)";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unexpected token: unexpected token; expected a child expression or closing delimiter
      |
    1 | (call 'foo)
      |       ^^^^^
    error: unclosed tree: unclosed tree; expected ')'
      |
    1 | (call 'foo)
      | -          ^
      | |
      | tree started here
    ");
}
