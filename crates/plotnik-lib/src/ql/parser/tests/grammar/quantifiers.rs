use crate::Query;
use indoc::indoc;

#[test]
fn quantifier_star() {
    let input = indoc! {r#"
    (statement)*
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Quantifier
          Tree
            ParenOpen "("
            Id "statement"
            ParenClose ")"
          Star "*"
    "#);
}

#[test]
fn quantifier_plus() {
    let input = indoc! {r#"
    (statement)+
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Quantifier
          Tree
            ParenOpen "("
            Id "statement"
            ParenClose ")"
          Plus "+"
    "#);
}

#[test]
fn quantifier_optional() {
    let input = indoc! {r#"
    (statement)?
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Quantifier
          Tree
            ParenOpen "("
            Id "statement"
            ParenClose ")"
          Question "?"
    "#);
}

#[test]
fn quantifier_with_capture() {
    let input = indoc! {r#"
    (statement)* @statements
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Quantifier
            Tree
              ParenOpen "("
              Id "statement"
              ParenClose ")"
            Star "*"
          At "@"
          Id "statements"
    "#);
}

#[test]
fn quantifier_inside_node() {
    let input = indoc! {r#"
    (block
        (statement)*)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "block"
          Quantifier
            Tree
              ParenOpen "("
              Id "statement"
              ParenClose ")"
            Star "*"
          ParenClose ")"
    "#);
}
