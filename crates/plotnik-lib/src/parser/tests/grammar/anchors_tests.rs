use crate::Query;
use indoc::indoc;

#[test]
fn anchor_first_child() {
    let input = indoc! {r#"
    Q = (block . (first_statement))
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = (block (last_statement) .)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = (dotted_name (identifier) @a . (identifier) @b)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = (array . (element) .)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = (tuple . (a) . (b) . (c) .)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = {. (first) (second) .}
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
