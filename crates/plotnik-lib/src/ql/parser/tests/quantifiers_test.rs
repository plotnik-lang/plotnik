use super::helpers_test::*;
use indoc::indoc;

#[test]
fn quantifier_star() {
    let input = indoc! {r#"
    (statement)*
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Quantifier
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        Star "*"
    "#);
}

#[test]
fn quantifier_plus() {
    let input = indoc! {r#"
    (statement)+
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Quantifier
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        Plus "+"
    "#);
}

#[test]
fn quantifier_optional() {
    let input = indoc! {r#"
    (statement)?
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Quantifier
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        Question "?"
    "#);
}

#[test]
fn quantifier_with_capture() {
    let input = indoc! {r#"
    (statement)* @statements
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Quantifier
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        Star "*"
      Capture
        At "@"
        CaptureName "statements"
    "#);
}

#[test]
fn quantifier_inside_node() {
    let input = indoc! {r#"
    (block
        (statement)*)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "block"
        Quantifier
          NamedNode
            ParenOpen "("
            LowerIdent "statement"
            ParenClose ")"
          Star "*"
        ParenClose ")"
    "#);
}