use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

#[test]
fn anchor_first_child() {
    let input = indoc! {r#"
    (block . (first_statement))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "block"
        Anchor
          Dot "."
        Node
          ParenOpen "("
          LowerIdent "first_statement"
          ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn anchor_last_child() {
    let input = indoc! {r#"
    (block (last_statement) .)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "block"
        Node
          ParenOpen "("
          LowerIdent "last_statement"
          ParenClose ")"
        Anchor
          Dot "."
        ParenClose ")"
    "#);
}

#[test]
fn anchor_adjacency() {
    let input = indoc! {r#"
    (dotted_name (identifier) @a . (identifier) @b)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "dotted_name"
        Node
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        Capture
          At "@"
          LowerIdent "a"
        Anchor
          Dot "."
        Node
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        Capture
          At "@"
          LowerIdent "b"
        ParenClose ")"
    "#);
}

#[test]
fn anchor_both_ends() {
    let input = indoc! {r#"
    (array . (element) .)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "array"
        Anchor
          Dot "."
        Node
          ParenOpen "("
          LowerIdent "element"
          ParenClose ")"
        Anchor
          Dot "."
        ParenClose ")"
    "#);
}

#[test]
fn anchor_multiple_adjacent() {
    let input = indoc! {r#"
    (tuple . (a) . (b) . (c) .)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "tuple"
        Anchor
          Dot "."
        Node
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        Anchor
          Dot "."
        Node
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
        Anchor
          Dot "."
        Node
          ParenOpen "("
          LowerIdent "c"
          ParenClose ")"
        Anchor
          Dot "."
        ParenClose ")"
    "#);
}

#[test]
fn anchor_in_sequence() {
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

#[test]
fn capture_space_after_dot_is_anchor() {
    let input = indoc! {r#"
    (identifier) @foo . (other)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "foo"
      Anchor
        Dot "."
      Node
        ParenOpen "("
        LowerIdent "other"
        ParenClose ")"
    "#);
}