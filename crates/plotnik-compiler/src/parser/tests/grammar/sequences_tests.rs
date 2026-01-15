//! Sequence parsing tests.

use crate::{shot_cst, shot_error, Query};

#[test]
fn treesitter_sequence_parses_as_seq() {
    let input = "Q = ((a) (b))";
    let res = Query::expect_cst_with_warnings(input);
    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Seq
          ParenOpen "("
          Tree
            ParenOpen "("
            Id "a"
            ParenClose ")"
          Tree
            ParenOpen "("
            Id "b"
            ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn treesitter_single_item_sequence_parses_as_seq() {
    let input = "Q = ((expression_statement))";
    let res = Query::expect_cst_with_warnings(input);
    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Seq
          ParenOpen "("
          Tree
            ParenOpen "("
            Id "expression_statement"
            ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn named_node_with_child_remains_tree() {
    shot_cst!(r#"
        Q = (foo (bar))
    "#);
}

#[test]
fn simple_sequence() {
    shot_cst!(r#"
        Q = {(a) (b)}
    "#);
}

#[test]
fn empty_sequence() {
    shot_error!(r#"
        Q = {}
    "#);
}

#[test]
fn sequence_single_element() {
    shot_cst!(r#"
        Q = {(identifier)}
    "#);
}

#[test]
fn sequence_with_captures() {
    shot_cst!(r#"
        Q = {(comment)* @comments (function) @fn}
    "#);
}

#[test]
fn sequence_with_quantifier() {
    let input = "Q = {(a) (b)}+";
    let res = Query::parse_cst(input);
    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Quantifier
          Seq
            BraceOpen "{"
            Tree
              ParenOpen "("
              Id "a"
              ParenClose ")"
            Tree
              ParenOpen "("
              Id "b"
              ParenClose ")"
            BraceClose "}"
          Plus "+"
    "#);
}

#[test]
fn nested_sequences() {
    shot_cst!(r#"
        Q = {{(a)} {(b)}}
    "#);
}

#[test]
fn sequence_in_named_node() {
    shot_cst!(r#"
        Q = (block {(statement) (statement)})
    "#);
}

#[test]
fn sequence_with_alternation() {
    shot_cst!(r#"
        Q = {[(a) (b)] (c)}
    "#);
}

#[test]
fn sequence_comma_separated_expression() {
    let input = r#"Q = {(number) {"," (number)}*}"#;
    let res = Query::parse_cst(input);
    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Seq
          BraceOpen "{"
          Tree
            ParenOpen "("
            Id "number"
            ParenClose ")"
          Quantifier
            Seq
              BraceOpen "{"
              Str
                DoubleQuote "\""
                StrVal ","
                DoubleQuote "\""
              Tree
                ParenOpen "("
                Id "number"
                ParenClose ")"
              BraceClose "}"
            Star "*"
          BraceClose "}"
    "#);
}

#[test]
fn sequence_with_anchor() {
    shot_cst!(r#"
        Q = (parent {. (first) (second) .})
    "#);
}
