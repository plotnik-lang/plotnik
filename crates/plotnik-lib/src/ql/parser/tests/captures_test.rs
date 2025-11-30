use super::helpers_test::*;
use indoc::indoc;

#[test]
fn capture() {
    let input = indoc! {r#"
    (identifier) @name
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "name"
    "#);
}

#[test]
fn capture_nested() {
    let input = indoc! {r#"
    (call function: (identifier) @func)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "function"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        Capture
          At "@"
          LowerIdent "func"
        ParenClose ")"
    "#);
}

#[test]
fn multiple_captures() {
    let input = indoc! {r#"
    (binary
        left: (_) @left
        right: (_) @right) @expr
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "binary"
        Field
          LowerIdent "left"
          Colon ":"
          Node
            ParenOpen "("
            Underscore "_"
            ParenClose ")"
        Capture
          At "@"
          LowerIdent "left"
        Field
          LowerIdent "right"
          Colon ":"
          Node
            ParenOpen "("
            Underscore "_"
            ParenClose ")"
        Capture
          At "@"
          LowerIdent "right"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "expr"
    "#);
}
