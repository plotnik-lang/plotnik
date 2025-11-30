use super::helpers_test::*;
use indoc::indoc;

#[test]
fn error_node() {
    let input = indoc! {r#"
    (ERROR)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
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
      NamedNode
        ParenOpen "("
        KwError "ERROR"
        ParenClose ")"
      Capture
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
      NamedNode
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
      NamedNode
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
      NamedNode
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
      NamedNode
        ParenOpen "("
        KwMissing "MISSING"
        StringLit "\";\""
        ParenClose ")"
      Capture
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
      Alternation
        BracketOpen "["
        NamedNode
          ParenOpen "("
          KwError "ERROR"
          ParenClose ")"
        NamedNode
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
      Sequence
        BraceOpen "{"
        NamedNode
          ParenOpen "("
          KwMissing "MISSING"
          StringLit "\";\""
          ParenClose ")"
        NamedNode
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
      NamedNode
        ParenOpen "("
        LowerIdent "function_definition"
        Field
          LowerIdent "body"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "block"
            NamedNode
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
        NamedNode
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
        NamedNode
          ParenOpen "("
          KwMissing "MISSING"
          LowerIdent "identifier"
          ParenClose ")"
        Question "?"
    "#);
}

#[test]
fn error_with_unexpected_content() {
    let input = indoc! {r#"
    (ERROR (something))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
        ParenOpen "("
        KwError "ERROR"
        NamedNode
          ParenOpen "("
          LowerIdent "something"
          ParenClose ")"
        ParenClose ")"
    ---
    error: (ERROR) takes no arguments
      |
    1 | (ERROR (something))
      |        ^
    "#);
}

#[test]
fn bare_error_keyword() {
    let input = indoc! {r#"
    ERROR
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Error
        KwError "ERROR"
    ---
    error: ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)
      |
    1 | ERROR
      | ^^^^^
    "#);
}

#[test]
fn bare_missing_keyword() {
    let input = indoc! {r#"
    MISSING
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Error
        KwMissing "MISSING"
    ---
    error: ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)
      |
    1 | MISSING
      | ^^^^^^^
    "#);
}