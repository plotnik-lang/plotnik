use crate::Query;
use indoc::indoc;

#[test]
fn missing_paren() {
    let input = indoc! {r#"
    (identifier
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed tree; expected ')'
      |
    1 | (identifier
      | -          ^ unclosed tree; expected ')'
      | |
      | tree started here
    ");
}

#[test]
fn missing_bracket() {
    let input = indoc! {r#"
    [(identifier) (string)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed alternation; expected ']'
      |
    1 | [(identifier) (string)
      | -                     ^ unclosed alternation; expected ']'
      | |
      | alternation started here
    ");
}

#[test]
fn missing_brace() {
    let input = indoc! {r#"
    {(a) (b)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed sequence; expected '}'
      |
    1 | {(a) (b)
      | -       ^ unclosed sequence; expected '}'
      | |
      | sequence started here
    ");
}

#[test]
fn nested_unclosed() {
    let input = indoc! {r#"
    (a (b (c)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed tree; expected ')'
      |
    1 | (a (b (c)
      |    -     ^ unclosed tree; expected ')'
      |    |
      |    tree started here
    ");
}

#[test]
fn deeply_nested_unclosed() {
    let input = indoc! {r#"
    (a (b (c (d
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed tree; expected ')'
      |
    1 | (a (b (c (d
      |          - ^ unclosed tree; expected ')'
      |          |
      |          tree started here
    ");
}

#[test]
fn unclosed_alternation_nested() {
    let input = indoc! {r#"
    [(a) (b
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed tree; expected ')'
      |
    1 | [(a) (b
      |      - ^ unclosed tree; expected ')'
      |      |
      |      tree started here
    ");
}

#[test]
fn empty_parens() {
    let input = indoc! {r#"
    ()
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: empty tree expression - expected node type or children
      |
    1 | ()
      |  ^ empty tree expression - expected node type or children
    ");
}

#[test]
fn unclosed_tree_shows_open_location() {
    let input = indoc! {r#"
    (call
        (identifier)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed tree; expected ')'
      |
    1 | (call
      | - tree started here
    2 |     (identifier)
      |                 ^ unclosed tree; expected ')'
    ");
}

#[test]
fn unclosed_alternation_shows_open_location() {
    let input = indoc! {r#"
    [
        (a)
        (b)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed alternation; expected ']'
      |
    1 | [
      | - alternation started here
    2 |     (a)
    3 |     (b)
      |        ^ unclosed alternation; expected ']'
    ");
}

#[test]
fn unclosed_sequence_shows_open_location() {
    let input = indoc! {r#"
    {
        (a)
        (b)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unclosed sequence; expected '}'
      |
    1 | {
      | - sequence started here
    2 |     (a)
    3 |     (b)
      |        ^ unclosed sequence; expected '}'
    ");
}

#[test]
fn unclosed_double_quote_string() {
    let input = r#"(call "foo)"#;

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | (call "foo)
      |       ^^^^^ unexpected token; expected a child expression or closing delimiter
    error: unclosed tree; expected ')'
      |
    1 | (call "foo)
      | -          ^ unclosed tree; expected ')'
      | |
      | tree started here
    "#);
}

#[test]
fn unclosed_single_quote_string() {
    let input = "(call 'foo)";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | (call 'foo)
      |       ^^^^^ unexpected token; expected a child expression or closing delimiter
    error: unclosed tree; expected ')'
      |
    1 | (call 'foo)
      | -          ^ unclosed tree; expected ')'
      | |
      | tree started here
    ");
}
