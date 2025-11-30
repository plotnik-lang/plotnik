use super::helpers_test::*;
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
        Node
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        Node
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
        Node
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
        Quantifier
          Node
            ParenOpen "("
            LowerIdent "comment"
            ParenClose ")"
          Star "*"
        Capture
          At "@"
          LowerIdent "comments"
        Node
          ParenOpen "("
          LowerIdent "function"
          ParenClose ")"
        Capture
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
          Node
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
          Node
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
          Node
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
          BraceClose "}"
        Seq
          BraceOpen "{"
          Node
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
      Node
        ParenOpen "("
        LowerIdent "block"
        Seq
          BraceOpen "{"
          Node
            ParenOpen "("
            LowerIdent "statement"
            ParenClose ")"
          Node
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
          Node
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
          Node
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
          BracketClose "]"
        Node
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
        Node
          ParenOpen "("
          LowerIdent "number"
          ParenClose ")"
        Quantifier
          Seq
            BraceOpen "{"
            Lit
              StringLit "\",\""
            Node
              ParenOpen "("
              LowerIdent "number"
              ParenClose ")"
            BraceClose "}"
          Star "*"
        BraceClose "}"
    "#);
}

#[test]
fn sequence_missing_close_brace() {
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
        Node
          ParenOpen "("
          LowerIdent "first"
          ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "second"
          ParenClose ")"
        Anchor
          Dot "."
        BraceClose "}"
    "#);
}
