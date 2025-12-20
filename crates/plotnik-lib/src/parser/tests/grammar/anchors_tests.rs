use crate::Query;
use indoc::indoc;

#[test]
fn anchor_first_child() {
    let input = indoc! {r#"
    Q = (block . (first_statement))
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
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

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
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

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
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

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
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

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
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

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
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
