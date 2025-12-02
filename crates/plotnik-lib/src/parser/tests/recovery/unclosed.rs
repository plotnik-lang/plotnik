use crate::Query;
use indoc::indoc;

#[test]
fn missing_paren() {
    let input = indoc! {r#"
    (identifier
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "identifier"
    ---
    error: unclosed tree; expected ')'
      |
    1 | (identifier
      | -          ^ unclosed tree; expected ')'
      | |
      | tree started here
    "#);
}

#[test]
fn missing_bracket() {
    let input = indoc! {r#"
    [(identifier) (string)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Branch
            Tree
              ParenOpen "("
              Id "identifier"
              ParenClose ")"
          Branch
            Tree
              ParenOpen "("
              Id "string"
              ParenClose ")"
    ---
    error: unclosed alternation; expected ']'
      |
    1 | [(identifier) (string)
      | -                     ^ unclosed alternation; expected ']'
      | |
      | alternation started here
    "#);
}

#[test]
fn missing_brace() {
    let input = indoc! {r#"
    {(a) (b)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Seq
          BraceOpen "{"
          Tree
            ParenOpen "("
            Id "a"
            ParenClose ")"
          Tree
            ParenOpen "("
            Id "b"
            ParenClose ")"
    ---
    error: unclosed sequence; expected '}'
      |
    1 | {(a) (b)
      | -       ^ unclosed sequence; expected '}'
      | |
      | sequence started here
    "#);
}

#[test]
fn nested_unclosed() {
    let input = indoc! {r#"
    (a (b (c)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "a"
          Tree
            ParenOpen "("
            Id "b"
            Tree
              ParenOpen "("
              Id "c"
              ParenClose ")"
    ---
    error: unclosed tree; expected ')'
      |
    1 | (a (b (c)
      |    -     ^ unclosed tree; expected ')'
      |    |
      |    tree started here
    "#);
}

#[test]
fn deeply_nested_unclosed() {
    let input = indoc! {r#"
    (a (b (c (d
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "a"
          Tree
            ParenOpen "("
            Id "b"
            Tree
              ParenOpen "("
              Id "c"
              Tree
                ParenOpen "("
                Id "d"
    ---
    error: unclosed tree; expected ')'
      |
    1 | (a (b (c (d
      |          - ^ unclosed tree; expected ')'
      |          |
      |          tree started here
    "#);
}

#[test]
fn unclosed_alternation_nested() {
    let input = indoc! {r#"
    [(a) (b
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Branch
            Tree
              ParenOpen "("
              Id "a"
              ParenClose ")"
          Branch
            Tree
              ParenOpen "("
              Id "b"
    ---
    error: unclosed tree; expected ')'
      |
    1 | [(a) (b
      |      - ^ unclosed tree; expected ')'
      |      |
      |      tree started here
    "#);
}

#[test]
fn empty_parens() {
    let input = indoc! {r#"
    ()
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          ParenClose ")"
    ---
    error: empty tree expression - expected node type or children
      |
    1 | ()
      |  ^ empty tree expression - expected node type or children
    "#);
}
