use crate::Query;
use indoc::indoc;

#[test]
fn quantifier_star() {
    let input = indoc! {r#"
    Q = (statement)*
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = (statement)+
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = (statement)?
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = (statement)* @statements
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = (block
        (statement)*)
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
          Quantifier
            Tree
              ParenOpen "("
              Id "statement"
              ParenClose ")"
            Star "*"
          ParenClose ")"
    "#);
}
