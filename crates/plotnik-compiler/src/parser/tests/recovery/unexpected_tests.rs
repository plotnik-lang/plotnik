use crate::Query;
use indoc::indoc;

#[test]
fn unexpected_token() {
    let input = indoc! {r#"
    (identifier) ^^^ (string)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token
      |
    1 | (identifier) ^^^ (string)
      |              ^^^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
    "#);
}

#[test]
fn multiple_consecutive_garbage() {
    let input = indoc! {r#"
    ^^^ $$$ %%% (ok)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token
      |
    1 | ^^^ $$$ %%% (ok)
      | ^^^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
    "#);
}

#[test]
fn garbage_at_start() {
    let input = indoc! {r#"
    ^^^ (a)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token
      |
    1 | ^^^ (a)
      | ^^^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
    "#);
}

#[test]
fn only_garbage() {
    let input = indoc! {r#"
    ^^^ $$$
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token
      |
    1 | ^^^ $$$
      | ^^^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
    "#);
}

#[test]
fn garbage_inside_alternation() {
    let input = indoc! {r#"
    [(a) ^^^ (b)]
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: unexpected token
      |
    1 | [(a) ^^^ (b)]
      |      ^^^
      |
    help: try `(node)` or close with `]`
    ");
}

#[test]
fn garbage_inside_node() {
    let input = indoc! {r#"
    (a (b) @@@ (c)) (d)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: unexpected token
      |
    1 | (a (b) @@@ (c)) (d)
      |        ^
      |
    help: try `(child)` or close with `)`

    error: unexpected token
      |
    1 | (a (b) @@@ (c)) (d)
      |          ^
      |
    help: try `(child)` or close with `)`
    ");
}

#[test]
fn xml_tag_garbage() {
    let input = indoc! {r#"
    <div>(identifier)</div>
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token
      |
    1 | <div>(identifier)</div>
      | ^^^^^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`

    error: unexpected token
      |
    1 | <div>(identifier)</div>
      |                  ^^^^^^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
    "#);
}

#[test]
fn xml_self_closing() {
    let input = indoc! {r#"
    <br/> (a)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token
      |
    1 | <br/> (a)
      | ^^^^^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
    "#);
}

#[test]
fn predicate_unsupported() {
    let input = indoc! {r#"
    (a (#eq? @x "foo") b)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: predicates are not supported
      |
    1 | (a (#eq? @x "foo") b)
      |     ^^^^

    error: unexpected token
      |
    1 | (a (#eq? @x "foo") b)
      |          ^^
      |
    help: try `(child)` or close with `)`

    error: bare identifier is not valid
      |
    1 | (a (#eq? @x "foo") b)
      |                    ^
      |
    help: wrap in parentheses
      |
    1 - (a (#eq? @x "foo") b)
    1 + (a (#eq? @x "foo") (b))
      |
    "#);
}

#[test]
fn predicate_match() {
    let input = indoc! {r#"
    (identifier) #match? @name "test"
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: predicates are not supported
      |
    1 | (identifier) #match? @name "test"
      |              ^^^^^^^
    "#);
}

#[test]
fn predicate_in_tree() {
    let input = "(function #eq? @name \"test\")";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: predicates are not supported
      |
    1 | (function #eq? @name "test")
      |           ^^^^

    error: unexpected token
      |
    1 | (function #eq? @name "test")
      |                ^^^^^
      |
    help: try `(child)` or close with `)`
    "#);
}

#[test]
fn predicate_in_alternation() {
    let input = indoc! {r#"
    [(a) #eq? (b)]
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: unexpected token
      |
    1 | [(a) #eq? (b)]
      |      ^^^^
      |
    help: try `(node)` or close with `]`
    ");
}

#[test]
fn predicate_in_sequence() {
    let input = indoc! {r#"
    {(a) #set! (b)}
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: predicates are not supported
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: unexpected token
      |
    2 | ^^^
      | ^^^
      |
    help: try `(child)` or close with `)`

    error: bare identifier is not valid
      |
    3 | b)
      | ^
      |
    help: wrap in parentheses
      |
    3 - b)
    3 + (b))
      |
    ");
}

#[test]
fn top_level_garbage_recovery() {
    let input = indoc! {r#"
    Expr = (a) ^^^ Expr2 = (b)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token
      |
    1 | Expr = (a) ^^^ Expr2 = (b)
      |            ^^^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token
      |
    2 | ^^^
      | ^^^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`

    error: unexpected token
      |
    4 | $$$
      | ^^^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
    "#);
}

#[test]
fn alternation_recovery_to_capture() {
    let input = indoc! {r#"
    [^^^ @name]
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: empty `[]` is not allowed
      |
    1 | [^^^ @name]
      | ^^^^^^^^^^^
      |
    help: alternations must contain at least one branch

    error: unexpected token
      |
    1 | [^^^ @name]
      |  ^^^
      |
    help: try `(node)` or close with `]`

    error: unexpected token
      |
    1 | [^^^ @name]
      |      ^^^^^
      |
    help: try `(node)` or close with `]`
    ");
}

#[test]
fn comma_between_defs() {
    let input = indoc! {r#"
    A = (a), B = (b)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token
      |
    1 | A = (a), B = (b)
      |        ^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
    "#);
}

#[test]
fn bare_colon_in_tree() {
    let input = "(a : (b))";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: unexpected token
      |
    1 | (a : (b))
      |    ^
      |
    help: try `(child)` or close with `)`
    ");
}

#[test]
fn paren_close_inside_alternation() {
    let input = "[(a) ) (b)]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token: expected closing ']' for alternation
      |
    1 | [(a) ) (b)]
      |      ^

    error: unexpected token
      |
    1 | [(a) ) (b)]
      |           ^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
    "#);
}

#[test]
fn bracket_close_inside_sequence() {
    let input = "{(a) ] (b)}";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token: expected closing '}' for sequence
      |
    1 | {(a) ] (b)}
      |      ^

    error: unexpected token
      |
    1 | {(a) ] (b)}
      |           ^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
    "#);
}

#[test]
fn paren_close_inside_sequence() {
    let input = "{(a) ) (b)}";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token: expected closing '}' for sequence
      |
    1 | {(a) ) (b)}
      |      ^

    error: unexpected token
      |
    1 | {(a) ) (b)}
      |           ^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
    "#);
}

#[test]
fn single_colon_type_annotation_followed_by_non_id() {
    let input = "(a) @x : (b)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token
      |
    1 | (a) @x : (b)
      |        ^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
    "#);
}

#[test]
fn single_colon_type_annotation_at_eof() {
    let input = "(a) @x :";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token
      |
    1 | (a) @x :
      |        ^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
    "#);
}
