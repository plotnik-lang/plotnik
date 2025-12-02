use crate::Query;
use indoc::indoc;

#[test]
fn missing_paren() {
    let input = indoc! {r#"
    (identifier
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected closing ')' for tree
      |
    1 | (identifier
      |            ^ expected closing ')' for tree
    ");
}

#[test]
fn missing_bracket() {
    let input = indoc! {r#"
    [(identifier) (string)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: unclosed alternation; expected ']'
      |
    1 | [(identifier) (string)
      | -                     ^ unclosed alternation; expected ']'
      | |
      | alternation started here
    "#);
}

#[test]
fn missing_brace() {
    let input = indoc! {r#"
    {(a) (b)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected closing '}' for sequence
      |
    1 | {(a) (b)
      |         ^ expected closing '}' for sequence
    ");
}

#[test]
fn nested_unclosed() {
    let input = indoc! {r#"
    (a (b (c)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected closing ')' for tree
      |
    1 | (a (b (c)
      |          ^ expected closing ')' for tree
    ");
}

#[test]
fn deeply_nested_unclosed() {
    let input = indoc! {r#"
    (a (b (c (d
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected closing ')' for tree
      |
    1 | (a (b (c (d
      |            ^ expected closing ')' for tree
    ");
}

#[test]
fn unclosed_alternation_nested() {
    let input = indoc! {r#"
    [(a) (b
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected closing ')' for tree
      |
    1 | [(a) (b
      |        ^ expected closing ')' for tree
    ");
}

#[test]
fn empty_parens() {
    let input = indoc! {r#"
    ()
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: empty tree expression - expected node type or children
      |
    1 | ()
      |  ^ empty tree expression - expected node type or children
    "#);
}
