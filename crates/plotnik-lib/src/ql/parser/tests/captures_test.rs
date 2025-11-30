use super::helpers_test::*;
use indoc::indoc;

#[test]
fn capture() {
    let input = indoc! {r#"
    (identifier) @name
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        CaptureName "name"
    "#);
}

#[test]
fn capture_dotted() {
    // Dotted captures like @var.name are lexed as a single CaptureName token
    let input = indoc! {r#"
    (identifier) @var.name
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        CaptureName "var.name"
    "#);
}

#[test]
fn capture_nested() {
    let input = indoc! {r#"
    (call function: (identifier) @func)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "function"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        Capture
          At "@"
          CaptureName "func"
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
      NamedNode
        ParenOpen "("
        LowerIdent "binary"
        Field
          LowerIdent "left"
          Colon ":"
          NamedNode
            ParenOpen "("
            Underscore "_"
            ParenClose ")"
        Capture
          At "@"
          CaptureName "left"
        Field
          LowerIdent "right"
          Colon ":"
          NamedNode
            ParenOpen "("
            Underscore "_"
            ParenClose ")"
        Capture
          At "@"
          CaptureName "right"
        ParenClose ")"
      Capture
        At "@"
        CaptureName "expr"
    "#);
}