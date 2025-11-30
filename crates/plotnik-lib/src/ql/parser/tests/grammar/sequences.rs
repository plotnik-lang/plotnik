use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

#[test]
fn simple_sequence() {
    let input = indoc! {r#"
    {(a) (b)}
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Seq
        BraceOpen "{"
        Tree
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        Tree
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
        BraceClose "}"
    "#);
}

#[test]
fn empty_sequence() {
    let input = indoc! {r#"
    {}
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Seq
        BraceOpen "{"
        BraceClose "}"
    "#);
}

#[test]
fn sequence_single_element() {
    let input = indoc! {r#"
    {(identifier)}
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Seq
        BraceOpen "{"
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        BraceClose "}"
    "#);
}

#[test]
fn sequence_with_captures() {
    let input = indoc! {r#"
    {(comment)* @comments (function) @fn}
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Seq
        BraceOpen "{"
        Capture
          Quantifier
            Tree
              ParenOpen "("
              LowerIdent "comment"
              ParenClose ")"
            Star "*"
          At "@"
          LowerIdent "comments"
        Capture
          Tree
            ParenOpen "("
            LowerIdent "function"
            ParenClose ")"
          At "@"
          LowerIdent "fn"
        BraceClose "}"
    "#);
}

#[test]
fn sequence_with_quantifier() {
    let input = indoc! {r#"
    {(a) (b)}+
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Quantifier
        Seq
          BraceOpen "{"
          Tree
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
          BraceClose "}"
        Plus "+"
    "#);
}

#[test]
fn nested_sequences() {
    let input = indoc! {r#"
    {{(a)} {(b)}}
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Seq
        BraceOpen "{"
        Seq
          BraceOpen "{"
          Tree
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
          BraceClose "}"
        Seq
          BraceOpen "{"
          Tree
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
          BraceClose "}"
        BraceClose "}"
    "#);
}

#[test]
fn sequence_in_named_node() {
    let input = indoc! {r#"
    (block {(statement) (statement)})
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Tree
        ParenOpen "("
        LowerIdent "block"
        Seq
          BraceOpen "{"
          Tree
            ParenOpen "("
            LowerIdent "statement"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "statement"
            ParenClose ")"
          BraceClose "}"
        ParenClose ")"
    "#);
}

#[test]
fn sequence_with_alternation() {
    let input = indoc! {r#"
    {[(a) (b)] (c)}
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Seq
        BraceOpen "{"
        Alt
          BracketOpen "["
          Tree
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
          BracketClose "]"
        Tree
          ParenOpen "("
          LowerIdent "c"
          ParenClose ")"
        BraceClose "}"
    "#);
}

#[test]
fn sequence_comma_separated_pattern() {
    let input = indoc! {r#"
    {(number) {"," (number)}*}
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Seq
        BraceOpen "{"
        Tree
          ParenOpen "("
          LowerIdent "number"
          ParenClose ")"
        Quantifier
          Seq
            BraceOpen "{"
            Lit
              StringLit "\",\""
            Tree
              ParenOpen "("
              LowerIdent "number"
              ParenClose ")"
            BraceClose "}"
          Star "*"
        BraceClose "}"
    "#);
}

#[test]
fn sequence_with_anchor() {
    let input = indoc! {r#"
    {. (first) (second) .}
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Seq
        BraceOpen "{"
        Anchor
          Dot "."
        Tree
          ParenOpen "("
          LowerIdent "first"
          ParenClose ")"
        Tree
          ParenOpen "("
          LowerIdent "second"
          ParenClose ")"
        Anchor
          Dot "."
        BraceClose "}"
    "#);
}
