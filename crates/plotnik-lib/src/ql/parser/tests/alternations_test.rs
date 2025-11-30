use super::helpers_test::*;
use indoc::indoc;

#[test]
fn alternation() {
    let input = indoc! {r#"
    [(identifier) (string)]
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alternation
        BracketOpen "["
        NamedNode
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        NamedNode
          ParenOpen "("
          LowerIdent "string"
          ParenClose ")"
        BracketClose "]"
    "#);
}

#[test]
fn alternation_with_anonymous() {
    let input = indoc! {r#"
    ["true" "false"]
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alternation
        BracketOpen "["
        AnonNode
          StringLit "\"true\""
        AnonNode
          StringLit "\"false\""
        BracketClose "]"
    "#);
}

#[test]
fn alternation_with_capture() {
    let input = indoc! {r#"
    [(identifier) (string)] @value
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alternation
        BracketOpen "["
        NamedNode
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        NamedNode
          ParenOpen "("
          LowerIdent "string"
          ParenClose ")"
        BracketClose "]"
      Capture
        At "@"
        LowerIdent "value"
    "#);
}

#[test]
fn alternation_nested() {
    let input = indoc! {r#"
    (expr
        [(binary) (unary)])
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "expr"
        Alternation
          BracketOpen "["
          NamedNode
            ParenOpen "("
            LowerIdent "binary"
            ParenClose ")"
          NamedNode
            ParenOpen "("
            LowerIdent "unary"
            ParenClose ")"
          BracketClose "]"
        ParenClose ")"
    "#);
}

#[test]
fn alternation_in_field() {
    let input = indoc! {r#"
    (call
        arguments: [(string) (number)])
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "arguments"
          Colon ":"
          Alternation
            BracketOpen "["
            NamedNode
              ParenOpen "("
              LowerIdent "string"
              ParenClose ")"
            NamedNode
              ParenOpen "("
              LowerIdent "number"
              ParenClose ")"
            BracketClose "]"
        ParenClose ")"
    "#);
}
