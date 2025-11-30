use crate::Query;
use indoc::indoc;

#[test]
fn anchor_first_child() {
    let input = indoc! {r#"
    (block . (first_statement))
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "block"
          Anchor
            Dot "."
          Tree
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "block"
          Tree
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "dotted_name"
          Capture
            Tree
              ParenOpen "("
              LowerIdent "identifier"
              ParenClose ")"
            At "@"
            LowerIdent "a"
          Anchor
            Dot "."
          Capture
            Tree
              ParenOpen "("
              LowerIdent "identifier"
              ParenClose ")"
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "array"
          Anchor
            Dot "."
          Tree
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "tuple"
          Anchor
            Dot "."
          Tree
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
          Anchor
            Dot "."
          Tree
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
          Anchor
            Dot "."
          Tree
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
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

#[test]
fn capture_space_after_dot_is_anchor() {
    let input = indoc! {r#"
    (identifier) @foo . (other)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          At "@"
          LowerIdent "foo"
      Def
        Anchor
          Dot "."
      Def
        Tree
          ParenOpen "("
          LowerIdent "other"
          ParenClose ")"
    ---
    error: unnamed definition must be last in file; add a name: `Name = (identifier) @foo`
      |
    1 | (identifier) @foo . (other)
      | ^^^^^^^^^^^^^^^^^
    error: unnamed definition must be last in file; add a name: `Name = .`
      |
    1 | (identifier) @foo . (other)
      |                   ^
    "#);
}
