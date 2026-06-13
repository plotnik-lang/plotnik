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

    insta::assert_snapshot!(res, @"
    error: unexpected token
      |
    1 | [(a) ^^^ (b)]
      |      ^^^
      |
    help: expected a branch, or `]` to close
    ");
}

#[test]
fn garbage_inside_node() {
    let input = indoc! {r#"
    (a (b) @@@ (c)) (d)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: unexpected token
      |
    1 | (a (b) @@@ (c)) (d)
      |        ^
      |
    help: expected a child node, or `)` to close

    error: unexpected token
      |
    1 | (a (b) @@@ (c)) (d)
      |          ^
      |
    help: expected a child node, or `)` to close
    ");
}

#[test]
fn predicate_unsupported() {
    let input = indoc! {r#"
    (a (#eq? @x "foo") b)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: tree-sitter predicates are not supported
      |
    1 | (a (#eq? @x "foo") b)
      |     ^^^^
      |
    help: use `(node == "x")`

    error: node types must be parenthesized
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
    error: tree-sitter predicates are not supported
      |
    1 | (identifier) #match? @name "test"
      |              ^^^^^^^
      |
    help: use `(node =~ /re/)`
    "#);
}

#[test]
fn predicate_in_tree() {
    let input = "(function #eq? @name \"test\")";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: tree-sitter predicates are not supported
      |
    1 | (function #eq? @name "test")
      |           ^^^^
      |
    help: use `(node == "x")`

    error: unexpected token
      |
    1 | (function #eq? @name "test")
      |                ^^^^^
      |
    help: expected a child node, or `)` to close
    "#);
}

#[test]
fn predicate_in_alternation() {
    let input = indoc! {r#"
    [(a) #eq? (b)]
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: tree-sitter predicates are not supported
      |
    1 | [(a) #eq? (b)]
      |      ^^^^
      |
    help: use `(node == "x")`
    "#);
}

#[test]
fn predicate_in_sequence() {
    let input = indoc! {r#"
    {(a) #set! (b)}
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: tree-sitter predicates are not supported
      |
    1 | {(a) #set! (b)}
      |      ^^^^^
    ");
}

#[test]
fn predicate_as_def_body() {
    let res = Query::expect_invalid("Q = #eq?");
    insta::assert_snapshot!(res, @r#"
    error: tree-sitter predicates are not supported
      |
    1 | Q = #eq?
      |     ^^^^
      |
    help: use `(node == "x")`
    "#);
}

#[test]
fn predicate_as_field_value() {
    let res = Query::expect_invalid("(call name: #eq?)");
    insta::assert_snapshot!(res, @r#"
    error: tree-sitter predicates are not supported
      |
    1 | (call name: #eq?)
      |             ^^^^
      |
    help: use `(node == "x")`
    "#);
}

#[test]
fn predicate_as_branch_value() {
    let res = Query::expect_invalid("[A: #eq? B: (b)]");
    insta::assert_snapshot!(res, @r#"
    error: tree-sitter predicates are not supported
      |
    1 | [A: #eq? B: (b)]
      |     ^^^^
      |
    help: use `(node == "x")`
    "#);
}

#[test]
fn predicate_not_eq_suggests_inequality() {
    let res = Query::expect_invalid("Q = #not-eq?");
    insta::assert_snapshot!(res, @r#"
    error: tree-sitter predicates are not supported
      |
    1 | Q = #not-eq?
      |     ^^^^^^^^
      |
    help: use `(node != "x")`
    "#);
}

#[test]
fn predicate_not_match_suggests_negated_regex() {
    let res = Query::expect_invalid("Q = #not-match?");
    insta::assert_snapshot!(res, @r#"
    error: tree-sitter predicates are not supported
      |
    1 | Q = #not-match?
      |     ^^^^^^^^^^^
      |
    help: use `(node !~ /re/)`
    "#);
}

#[test]
fn predicate_parenthesized_no_arg_cascade() {
    // The whole `(#eq? ...)` group is one error unit — its `@x "foo"` arguments do not
    // cascade into spurious child diagnostics.
    let res = Query::expect_invalid(r#"(call (#eq? @x "foo"))"#);
    insta::assert_snapshot!(res, @r#"
    error: tree-sitter predicates are not supported
      |
    1 | (call (#eq? @x "foo"))
      |        ^^^^
      |
    help: use `(node == "x")`
    "#);
}

#[test]
fn predicate_with_node_arg_balanced() {
    // The swallowed run tracks nested parens, so a node-pattern argument ends at the
    // predicate's own `)` instead of the inner one, leaving no outer `)` to cascade.
    let res = Query::expect_invalid("(call_expression (#eq? (identifier) @x))");
    insta::assert_snapshot!(res, @r#"
    error: tree-sitter predicates are not supported
      |
    1 | (call_expression (#eq? (identifier) @x))
      |                   ^^^^
      |
    help: use `(node == "x")`
    "#);
}

#[test]
fn multiline_garbage_recovery() {
    let input = indoc! {r#"
    (a
    ^^^
    b)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: unexpected token
      |
    2 | ^^^
      | ^^^
      |
    help: expected a child node, or `)` to close

    error: node types must be parenthesized
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

    insta::assert_snapshot!(res, @"
    error: empty `[]` matches nothing
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
    help: expected a branch, or `]` to close

    error: unexpected token
      |
    1 | [^^^ @name]
      |      ^^^^^
      |
    help: expected a branch, or `]` to close
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

    insta::assert_snapshot!(res, @"
    error: unexpected token
      |
    1 | (a : (b))
      |    ^
      |
    help: expected a child node, or `)` to close
    ");
}

#[test]
fn paren_close_inside_alternation() {
    let input = "[(a) ) (b)]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: expected closing ']' for alternation
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
    error: expected closing '}' for sequence
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
    error: expected closing '}' for sequence
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
