use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

#[test]
fn error_node() {
    let input = indoc! {r#"
    (ERROR)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Tree
        ParenOpen "("
        KwError "ERROR"
        ParenClose ")"
    "#);
}

#[test]
fn error_node_with_capture() {
    let input = indoc! {r#"
    (ERROR) @err
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Capture
        Tree
          ParenOpen "("
          KwError "ERROR"
          ParenClose ")"
        At "@"
        LowerIdent "err"
    "#);
}

#[test]
fn missing_node_bare() {
    let input = indoc! {r#"
    (MISSING)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Tree
        ParenOpen "("
        KwMissing "MISSING"
        ParenClose ")"
    "#);
}

#[test]
fn missing_node_with_type() {
    let input = indoc! {r#"
    (MISSING identifier)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Tree
        ParenOpen "("
        KwMissing "MISSING"
        LowerIdent "identifier"
        ParenClose ")"
    "#);
}

#[test]
fn missing_node_with_string() {
    let input = indoc! {r#"
    (MISSING ";")
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Tree
        ParenOpen "("
        KwMissing "MISSING"
        StringLit "\";\""
        ParenClose ")"
    "#);
}

#[test]
fn missing_node_with_capture() {
    let input = indoc! {r#"
    (MISSING ";") @missing_semi
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Capture
        Tree
          ParenOpen "("
          KwMissing "MISSING"
          StringLit "\";\""
          ParenClose ")"
        At "@"
        LowerIdent "missing_semi"
    "#);
}

#[test]
fn error_in_alternation() {
    let input = indoc! {r#"
    [(ERROR) (identifier)]
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Tree
          ParenOpen "("
          KwError "ERROR"
          ParenClose ")"
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        BracketClose "]"
    "#);
}

#[test]
fn missing_in_sequence() {
    let input = indoc! {r#"
    {(MISSING ";") (identifier)}
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Seq
        BraceOpen "{"
        Tree
          ParenOpen "("
          KwMissing "MISSING"
          StringLit "\";\""
          ParenClose ")"
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        BraceClose "}"
    "#);
}

#[test]
fn special_node_nested() {
    let input = indoc! {r#"
    (function_definition
        body: (block (ERROR)))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Tree
        ParenOpen "("
        LowerIdent "function_definition"
        Field
          LowerIdent "body"
          Colon ":"
          Tree
            ParenOpen "("
            LowerIdent "block"
            Tree
              ParenOpen "("
              KwError "ERROR"
              ParenClose ")"
            ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn error_with_quantifier() {
    let input = indoc! {r#"
    (ERROR)*
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Quantifier
        Tree
          ParenOpen "("
          KwError "ERROR"
          ParenClose ")"
        Star "*"
    "#);
}

#[test]
fn missing_with_quantifier() {
    let input = indoc! {r#"
    (MISSING identifier)?
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Quantifier
        Tree
          ParenOpen "("
          KwMissing "MISSING"
          LowerIdent "identifier"
          ParenClose ")"
        Question "?"
    "#);
}
