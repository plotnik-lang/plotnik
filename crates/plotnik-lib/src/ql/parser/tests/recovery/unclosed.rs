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
    error: unclosed node; expected ')'
      |
    1 | (identifier
      | -          ^
      | |
      | node started here
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
    error: unclosed alternation; expected ']'
      |
    1 | [(identifier) (string)
      | -                     ^
      | |
      | alternation started here
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
    error: unclosed sequence; expected '}'
      |
    1 | {(a) (b)
      | -       ^
      | |
      | sequence started here
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
    error: unclosed node; expected ')'
      |
    1 | (a (b (c)
      |    -     ^
      |    |
      |    node started here
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
    error: unclosed node; expected ')'
      |
    1 | (a (b (c (d
      |          - ^
      |          |
      |          node started here
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
    error: unclosed node; expected ')'
      |
    1 | [(a) (b
      |      - ^
      |      |
      |      node started here
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
