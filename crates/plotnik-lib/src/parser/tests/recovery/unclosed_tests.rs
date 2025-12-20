use crate::Query;
use indoc::indoc;

#[test]
fn missing_paren() {
    let input = indoc! {r#"
    (identifier
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: missing closing `)`; expected `)`
      |
    1 | (identifier
      | -^^^^^^^^^^
      | |
      | tree started here
    ");
}

#[test]
fn missing_bracket() {
    let input = indoc! {r#"
    [(identifier) (string)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: missing closing `]`; expected `]`
      |
    1 | [(identifier) (string)
      | -^^^^^^^^^^^^^^^^^^^^^
      | |
      | alternation started here
    ");
}

#[test]
fn missing_brace() {
    let input = indoc! {r#"
    {(a) (b)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: missing closing `}`; expected `}`
      |
    1 | {(a) (b)
      | -^^^^^^^
      | |
      | sequence started here
    ");
}

#[test]
fn nested_unclosed() {
    let input = indoc! {r#"
    (a (b (c)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: missing closing `)`; expected `)`
      |
    1 | (a (b (c)
      |    -^^^^^
      |    |
      |    tree started here
    ");
}

#[test]
fn deeply_nested_unclosed() {
    let input = indoc! {r#"
    (a (b (c (d
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: missing closing `)`; expected `)`
      |
    1 | (a (b (c (d
      |          -^
      |          |
      |          tree started here
    ");
}

#[test]
fn unclosed_alternation_nested() {
    let input = indoc! {r#"
    [(a) (b
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: missing closing `)`; expected `)`
      |
    1 | [(a) (b
      |      -^
      |      |
      |      tree started here
    ");
}

#[test]
fn empty_parens() {
    let input = indoc! {r#"
    ()
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: empty parentheses are not allowed
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: missing closing `)`; expected `)`
      |
    1 |   (call
      |   ^ tree started here
      |  _|
      | |
    2 | |     (identifier)
      | |_________________^
    ");
}

#[test]
fn unclosed_alternation_shows_open_location() {
    let input = indoc! {r#"
    [
        (a)
        (b)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: missing closing `]`; expected `]`
      |
    1 |   [
      |   ^ alternation started here
      |  _|
      | |
    2 | |     (a)
    3 | |     (b)
      | |________^
    ");
}

#[test]
fn unclosed_sequence_shows_open_location() {
    let input = indoc! {r#"
    {
        (a)
        (b)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: missing closing `}`; expected `}`
      |
    1 |   {
      |   ^ sequence started here
      |  _|
      | |
    2 | |     (a)
    3 | |     (b)
      | |________^
    ");
}

#[test]
fn unclosed_double_quote_string() {
    let input = r#"(call "foo)"#;

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: missing closing `)`; expected `)`
      |
    1 | (call "foo)
      | -^^^^^^^^^^
      | |
      | tree started here
    "#);
}

#[test]
fn unclosed_single_quote_string() {
    let input = "(call 'foo)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: missing closing `)`; expected `)`
      |
    1 | (call 'foo)
      | -^^^^^^^^^^
      | |
      | tree started here
    ");
}
