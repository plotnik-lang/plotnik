use crate::Query;
use indoc::indoc;

#[test]
fn anchor_first_child() {
    let input = indoc! {r#"
    (block . (first_statement))
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "block"
          Anchor
            Dot "."
          Tree
            ParenOpen "("
            Id "first_statement"
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
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "block"
          Tree
            ParenOpen "("
            Id "last_statement"
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
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "dotted_name"
          Capture
            Tree
              ParenOpen "("
              Id "identifier"
              ParenClose ")"
            At "@"
            Id "a"
          Anchor
            Dot "."
          Capture
            Tree
              ParenOpen "("
              Id "identifier"
              ParenClose ")"
            At "@"
            Id "b"
          ParenClose ")"
    "#);
}

#[test]
fn anchor_both_ends() {
    let input = indoc! {r#"
    (array . (element) .)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "array"
          Anchor
            Dot "."
          Tree
            ParenOpen "("
            Id "element"
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
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "tuple"
          Anchor
            Dot "."
          Tree
            ParenOpen "("
            Id "a"
            ParenClose ")"
          Anchor
            Dot "."
          Tree
            ParenOpen "("
            Id "b"
            ParenClose ")"
          Anchor
            Dot "."
          Tree
            ParenOpen "("
            Id "c"
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
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Seq
          BraceOpen "{"
          Anchor
            Dot "."
          Tree
            ParenOpen "("
            Id "first"
            ParenClose ")"
          Tree
            ParenOpen "("
            Id "second"
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
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            Id "identifier"
            ParenClose ")"
          At "@"
          Id "foo"
      Def
        Anchor
          Dot "."
      Def
        Tree
          ParenOpen "("
          Id "other"
          ParenClose ")"
    ---
    error: unnamed definition must be last in file; add a name: `Name = (identifier) @foo`
      |
    1 | (identifier) @foo . (other)
      | ^^^^^^^^^^^^^^^^^ unnamed definition must be last in file; add a name: `Name = (identifier) @foo`
    error: unnamed definition must be last in file; add a name: `Name = .`
      |
    1 | (identifier) @foo . (other)
      |                   ^ unnamed definition must be last in file; add a name: `Name = .`
    "#);
}
