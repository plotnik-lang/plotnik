use crate::Query;

#[test]
fn interior_anchor_always_valid() {
    let input = "Q = {(a) . (b)}";

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        Seq
          NamedNode a
          .
          NamedNode b
    ");
}

#[test]
fn anchor_inside_named_node_first() {
    let input = "Q = (parent . (first))";

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        NamedNode parent
          .
          NamedNode first
    ");
}

#[test]
fn anchor_inside_named_node_last() {
    let input = "Q = (parent (last) .)";

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        NamedNode parent
          NamedNode last
          .
    ");
}

#[test]
fn anchor_inside_named_node_both() {
    let input = "Q = (parent . (first) (second) .)";

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        NamedNode parent
          .
          NamedNode first
          NamedNode second
          .
    ");
}

#[test]
fn anchor_in_seq_inside_named_node() {
    let input = "Q = (parent {. (first)})";

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        NamedNode parent
          Seq
            .
            NamedNode first
    ");
}

#[test]
fn boundary_anchor_at_seq_start_without_context() {
    let input = "Q = {. (a)}";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: boundary anchor requires parent node context
      |
    1 | Q = {. (a)}
      |      ^
      |
    help: wrap in a named node: `(parent . (child))`
    ");
}

#[test]
fn boundary_anchor_at_seq_end_without_context() {
    let input = "Q = {(a) .}";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: boundary anchor requires parent node context
      |
    1 | Q = {(a) .}
      |          ^
      |
    help: wrap in a named node: `(parent . (child))`
    ");
}

#[test]
fn multiple_boundary_anchors_without_context() {
    let input = "Q = {. (a) .}";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: boundary anchor requires parent node context
      |
    1 | Q = {. (a) .}
      |      ^
      |
    help: wrap in a named node: `(parent . (child))`

    error: boundary anchor requires parent node context
      |
    1 | Q = {. (a) .}
      |            ^
      |
    help: wrap in a named node: `(parent . (child))`
    ");
}

#[test]
fn interior_anchor_with_alternation() {
    let input = "Q = {(a) . [(b) (c)]}";

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        Seq
          NamedNode a
          .
          Alt
            Branch
              NamedNode b
            Branch
              NamedNode c
    ");
}

#[test]
fn nested_named_node_provides_context() {
    let input = "Q = (outer (inner . (first)))";

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        NamedNode outer
          NamedNode inner
            .
            NamedNode first
    ");
}

// Parser-level: anchors in alternations

#[test]
fn anchor_in_alternation_error() {
    let input = "Q = [(a) . (b)]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: anchors cannot appear directly in alternations
      |
    1 | Q = [(a) . (b)]
      |          ^
      |
    help: use `[{(a) . (b)} (c)]` to anchor within a branch
    ");
}

#[test]
fn multiple_anchors_in_alternation_error() {
    let input = "Q = [. (a) . (b) .]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: anchors cannot appear directly in alternations
      |
    1 | Q = [. (a) . (b) .]
      |      ^
      |
    help: use `[{(a) . (b)} (c)]` to anchor within a branch

    error: anchors cannot appear directly in alternations
      |
    1 | Q = [. (a) . (b) .]
      |            ^
      |
    help: use `[{(a) . (b)} (c)]` to anchor within a branch

    error: anchors cannot appear directly in alternations
      |
    1 | Q = [. (a) . (b) .]
      |                  ^
      |
    help: use `[{(a) . (b)} (c)]` to anchor within a branch
    ");
}

#[test]
fn anchor_in_seq_inside_alt_ok() {
    let input = "Q = [{(a) . (b)} (c)]";

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        Alt
          Branch
            Seq
              NamedNode a
              .
              NamedNode b
          Branch
            NamedNode c
    ");
}
