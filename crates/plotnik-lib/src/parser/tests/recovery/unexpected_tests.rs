use crate::Query;
use indoc::indoc;

#[test]
fn unexpected_token() {
    let input = indoc! {r#"
    (identifier) ^^^ (string)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | (identifier) ^^^ (string)
      |              ^^^
    "#);
}

#[test]
fn multiple_consecutive_garbage() {
    let input = indoc! {r#"
    ^^^ $$$ %%% (ok)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | ^^^ $$$ %%% (ok)
      | ^^^
    "#);
}

#[test]
fn garbage_at_start() {
    let input = indoc! {r#"
    ^^^ (a)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | ^^^ (a)
      | ^^^
    "#);
}

#[test]
fn only_garbage() {
    let input = indoc! {r#"
    ^^^ $$$
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | ^^^ $$$
      | ^^^
    "#);
}

#[test]
fn garbage_inside_alternation() {
    let input = indoc! {r#"
    [(a) ^^^ (b)]
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unexpected token; not valid inside alternation — try `(node)` or close with `]`
      |
    1 | [(a) ^^^ (b)]
      |      ^^^
    ");
}

#[test]
fn garbage_inside_node() {
    let input = indoc! {r#"
    (a (b) @@@ (c)) (d)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected name after `@`
      |
    1 | (a (b) @@@ (c)) (d)
      |         ^
    ");
}

#[test]
fn xml_tag_garbage() {
    let input = indoc! {r#"
    <div>(identifier)</div>
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | <div>(identifier)</div>
      | ^^^^^

    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | <div>(identifier)</div>
      |                  ^^^^^^
    "#);
}

#[test]
fn xml_self_closing() {
    let input = indoc! {r#"
    <br/> (a)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | <br/> (a)
      | ^^^^^
    "#);
}

#[test]
fn predicate_unsupported() {
    let input = indoc! {r#"
    (a (#eq? @x "foo") b)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: predicates like `#match?` are not supported
      |
    1 | (a (#eq? @x "foo") b)
      |     ^^^^

    error: unexpected token; not valid inside a node — try `(child)` or close with `)`
      |
    1 | (a (#eq? @x "foo") b)
      |          ^

    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | (a (#eq? @x "foo") b)
      |                    ^
    "#);
}

#[test]
fn predicate_match() {
    let input = indoc! {r#"
    (identifier) #match? @name "test"
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: predicates like `#match?` are not supported
      |
    1 | (identifier) #match? @name "test"
      |              ^^^^^^^

    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | (identifier) #match? @name "test"
      |                       ^^^^
    "#);
}

#[test]
fn predicate_in_tree() {
    let input = "(function #eq? @name \"test\")";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: predicates like `#match?` are not supported
      |
    1 | (function #eq? @name "test")
      |           ^^^^

    error: unexpected token; not valid inside a node — try `(child)` or close with `)`
      |
    1 | (function #eq? @name "test")
      |                ^
    "#);
}

#[test]
fn predicate_in_alternation() {
    let input = indoc! {r#"
    [(a) #eq? (b)]
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unexpected token; not valid inside alternation — try `(node)` or close with `]`
      |
    1 | [(a) #eq? (b)]
      |      ^^^^
    ");
}

#[test]
fn predicate_in_sequence() {
    let input = indoc! {r#"
    {(a) #set! (b)}
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: predicates like `#match?` are not supported
      |
    1 | {(a) #set! (b)}
      |      ^^^^^
    ");
}

#[test]
fn multiline_garbage_recovery() {
    let input = indoc! {r#"
    (a
    ^^^
    b)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unexpected token; not valid inside a node — try `(child)` or close with `)`
      |
    2 | ^^^
      | ^^^

    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    3 | b)
      | ^
    ");
}

#[test]
fn top_level_garbage_recovery() {
    let input = indoc! {r#"
    Expr = (a) ^^^ Expr2 = (b)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | Expr = (a) ^^^ Expr2 = (b)
      |            ^^^
    "#);
}

#[test]
fn multiple_definitions_with_garbage_between() {
    let input = indoc! {r#"
    A = (a)
    ^^^
    B = (b)
    $$$
    C = (c)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    2 | ^^^
      | ^^^

    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    4 | $$$
      | ^^^
    "#);
}

#[test]
fn alternation_recovery_to_capture() {
    let input = indoc! {r#"
    [^^^ @name]
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unexpected token; not valid inside alternation — try `(node)` or close with `]`
      |
    1 | [^^^ @name]
      |  ^^^

    error: unexpected token; not valid inside alternation — try `(node)` or close with `]`
      |
    1 | [^^^ @name]
      |      ^
    ");
}

#[test]
fn comma_between_defs() {
    let input = indoc! {r#"
    A = (a), B = (b)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | A = (a), B = (b)
      |        ^
    "#);
}

#[test]
fn bare_colon_in_tree() {
    let input = "(a : (b))";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unexpected token; not valid inside a node — try `(child)` or close with `)`
      |
    1 | (a : (b))
      |    ^
    ");
}

#[test]
fn paren_close_inside_alternation() {
    let input = "[(a) ) (b)]";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; expected closing ']' for alternation
      |
    1 | [(a) ) (b)]
      |      ^

    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | [(a) ) (b)]
      |           ^
    "#);
}

#[test]
fn bracket_close_inside_sequence() {
    let input = "{(a) ] (b)}";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; expected closing '}' for sequence
      |
    1 | {(a) ] (b)}
      |      ^

    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | {(a) ] (b)}
      |           ^
    "#);
}

#[test]
fn paren_close_inside_sequence() {
    let input = "{(a) ) (b)}";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; expected closing '}' for sequence
      |
    1 | {(a) ) (b)}
      |      ^

    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | {(a) ) (b)}
      |           ^
    "#);
}

#[test]
fn single_colon_type_annotation_followed_by_non_id() {
    let input = "(a) @x : (b)";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | (a) @x : (b)
      |        ^
    "#);
}

#[test]
fn single_colon_type_annotation_at_eof() {
    let input = "(a) @x :";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | (a) @x :
      |        ^
    "#);
}
