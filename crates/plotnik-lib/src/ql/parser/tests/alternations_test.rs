use super::helpers_test::*;
use indoc::indoc;

#[test]
fn alternation() {
    let input = indoc! {r#"
    [(identifier) (string)]
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Node
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        Node
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
      Alt
        BracketOpen "["
        Lit
          StringLit "\"true\""
        Lit
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
      Alt
        BracketOpen "["
        Node
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        Node
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
      Node
        ParenOpen "("
        LowerIdent "expr"
        Alt
          BracketOpen "["
          Node
            ParenOpen "("
            LowerIdent "binary"
            ParenClose ")"
          Node
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
      Node
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "arguments"
          Colon ":"
          Alt
            BracketOpen "["
            Node
              ParenOpen "("
              LowerIdent "string"
              ParenClose ")"
            Node
              ParenOpen "("
              LowerIdent "number"
              ParenClose ")"
            BracketClose "]"
        ParenClose ")"
    "#);
}
