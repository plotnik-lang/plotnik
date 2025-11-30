use super::helpers_test::*;
use indoc::indoc;

#[test]
fn simple_sequence() {
    let input = indoc! {r#"
    {(a) (b)}
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Sequence
        BraceOpen "{"
        NamedNode
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        NamedNode
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
      Sequence
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
      Sequence
        BraceOpen "{"
        NamedNode
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
      Sequence
        BraceOpen "{"
        Quantifier
          NamedNode
            ParenOpen "("
            LowerIdent "comment"
            ParenClose ")"
          Star "*"
        Capture
          At "@"
          LowerIdent "comments"
        NamedNode
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
        Sequence
          BraceOpen "{"
          NamedNode
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
          NamedNode
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
      Sequence
        BraceOpen "{"
        Sequence
          BraceOpen "{"
          NamedNode
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
          BraceClose "}"
        Sequence
          BraceOpen "{"
          NamedNode
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
      NamedNode
        ParenOpen "("
        LowerIdent "block"
        Sequence
          BraceOpen "{"
          NamedNode
            ParenOpen "("
            LowerIdent "statement"
            ParenClose ")"
          NamedNode
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
      Sequence
        BraceOpen "{"
        Alternation
          BracketOpen "["
          NamedNode
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
          NamedNode
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
          BracketClose "]"
        NamedNode
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
      Sequence
        BraceOpen "{"
        NamedNode
          ParenOpen "("
          LowerIdent "number"
          ParenClose ")"
        Quantifier
          Sequence
            BraceOpen "{"
            AnonNode
              StringLit "\",\""
            NamedNode
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
      Sequence
        BraceOpen "{"
        NamedNode
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        NamedNode
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
      Sequence
        BraceOpen "{"
        Anchor
          Dot "."
        NamedNode
          ParenOpen "("
          LowerIdent "first"
          ParenClose ")"
        NamedNode
          ParenOpen "("
          LowerIdent "second"
          ParenClose ")"
        Anchor
          Dot "."
        BraceClose "}"
    "#);
}
