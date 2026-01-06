use crate::Query;

#[test]
fn empty_tree() {
    let input = "Q = ()";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: empty `()` is not allowed
      |
    1 | Q = ()
      |     ^^
      |
    help: use `(_)` to match any named node, or `_` for any node
    ");
}

#[test]
fn empty_tree_with_whitespace() {
    let input = "Q = (   )";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: empty `()` is not allowed
      |
    1 | Q = (   )
      |     ^^^^^
      |
    help: use `(_)` to match any named node, or `_` for any node
    ");
}

#[test]
fn empty_tree_with_comment() {
    let input = "Q = ( /* comment */ )";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: empty `()` is not allowed
      |
    1 | Q = ( /* comment */ )
      |     ^^^^^^^^^^^^^^^^^
      |
    help: use `(_)` to match any named node, or `_` for any node
    ");
}

#[test]
fn empty_sequence() {
    let input = "Q = {}";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: empty `{}` is not allowed
      |
    1 | Q = {}
      |     ^^
      |
    help: sequences must contain at least one expression
    ");
}

#[test]
fn empty_sequence_with_whitespace() {
    let input = "Q = {   }";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: empty `{}` is not allowed
      |
    1 | Q = {   }
      |     ^^^^^
      |
    help: sequences must contain at least one expression
    ");
}

#[test]
fn empty_sequence_with_comment() {
    let input = "Q = { /* comment */ }";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: empty `{}` is not allowed
      |
    1 | Q = { /* comment */ }
      |     ^^^^^^^^^^^^^^^^^
      |
    help: sequences must contain at least one expression
    ");
}

#[test]
fn empty_alternation() {
    let input = "Q = []";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: empty `[]` is not allowed
      |
    1 | Q = []
      |     ^^
      |
    help: alternations must contain at least one branch
    ");
}

#[test]
fn empty_alternation_with_whitespace() {
    let input = "Q = [   ]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: empty `[]` is not allowed
      |
    1 | Q = [   ]
      |     ^^^^^
      |
    help: alternations must contain at least one branch
    ");
}

#[test]
fn empty_alternation_with_comment() {
    let input = "Q = [ /* comment */ ]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: empty `[]` is not allowed
      |
    1 | Q = [ /* comment */ ]
      |     ^^^^^^^^^^^^^^^^^
      |
    help: alternations must contain at least one branch
    ");
}

#[test]
fn nested_empty_sequence() {
    let input = "Q = (foo {})";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: empty `{}` is not allowed
      |
    1 | Q = (foo {})
      |          ^^
      |
    help: sequences must contain at least one expression
    ");
}

#[test]
fn nested_empty_alternation() {
    let input = "Q = (foo [])";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: empty `[]` is not allowed
      |
    1 | Q = (foo [])
      |          ^^
      |
    help: alternations must contain at least one branch
    ");
}

#[test]
fn non_empty_sequence_valid() {
    let input = "Q = {(a) (b)}";

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        Seq
          NamedNode a
          NamedNode b
    ");
}

#[test]
fn non_empty_alternation_valid() {
    let input = "Q = [(a) (b)]";

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        Alt
          Branch
            NamedNode a
          Branch
            NamedNode b
    ");
}
