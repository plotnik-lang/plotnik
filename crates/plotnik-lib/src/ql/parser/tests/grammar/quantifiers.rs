use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

#[test]
fn quantifier_star() {
    let input = indoc! {r#"
    (statement)*
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Quantifier
        Tree
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
        Tree
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
        Tree
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
      Capture
        Quantifier
          Tree
            ParenOpen "("
            LowerIdent "statement"
            ParenClose ")"
          Star "*"
        At "@"
        LowerIdent "statements"
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
      Tree
        ParenOpen "("
        LowerIdent "block"
        Quantifier
          Tree
            ParenOpen "("
            LowerIdent "statement"
            ParenClose ")"
          Star "*"
        ParenClose ")"
    "#);
}
