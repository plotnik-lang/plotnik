use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

#[test]
fn missing_paren() {
    let input = indoc! {r#"
    (identifier
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
    ---
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | (identifier
      |            ^
    "#);
}

#[test]
fn missing_bracket() {
    let input = indoc! {r#"
    [(identifier) (string)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Node
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "string"
          ParenClose ")"
    ---
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | [(identifier) (string)
      |                       ^
    "#);
}

#[test]
fn missing_brace() {
    let input = indoc! {r#"
    {(a) (b)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Seq
        BraceOpen "{"
        Node
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
    ---
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | {(a) (b)
      |         ^
    "#);
}

#[test]
fn nested_unclosed() {
    let input = indoc! {r#"
    (a (b (c)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "a"
        Node
          ParenOpen "("
          LowerIdent "b"
          Node
            ParenOpen "("
            LowerIdent "c"
            ParenClose ")"
    ---
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | (a (b (c)
      |          ^
    "#);
}

#[test]
fn deeply_nested_unclosed() {
    let input = indoc! {r#"
    (a (b (c (d
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "a"
        Node
          ParenOpen "("
          LowerIdent "b"
          Node
            ParenOpen "("
            LowerIdent "c"
            Node
              ParenOpen "("
              LowerIdent "d"
    ---
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | (a (b (c (d
      |            ^
    "#);
}

#[test]
fn unclosed_alternation_nested() {
    let input = indoc! {r#"
    [(a) (b
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Node
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "b"
    ---
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | [(a) (b
      |        ^
    "#);
}

#[test]
fn empty_parens() {
    let input = indoc! {r#"
    ()
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        ParenClose ")"
    ---
    error: empty node pattern - expected node type or children
      |
    1 | ()
      |  ^
    "#);
}