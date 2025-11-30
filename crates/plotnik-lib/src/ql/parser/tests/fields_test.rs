use super::helpers_test::*;
use indoc::indoc;

#[test]
fn field_pattern() {
    let input = indoc! {r#"
    (call function: (identifier))
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
        ParenClose ")"
    "#);
}

#[test]
fn multiple_fields() {
    let input = indoc! {r#"
    (assignment
        left: (identifier)
        right: (expression))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "assignment"
        Field
          LowerIdent "left"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        Field
          LowerIdent "right"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "expression"
            ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn negated_field() {
    let input = indoc! {r#"
    (function !async)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "function"
        NegatedField
          Negation "!"
          LowerIdent "async"
        ParenClose ")"
    "#);
}

#[test]
fn negated_and_regular_fields() {
    let input = indoc! {r#"
    (function
        !async
        name: (identifier))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "function"
        NegatedField
          Negation "!"
          LowerIdent "async"
        Field
          LowerIdent "name"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn multiple_patterns() {
    let input = indoc! {r#"
    (identifier)
    (string)
    (number)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Node
        ParenOpen "("
        LowerIdent "string"
        ParenClose ")"
      Node
        ParenOpen "("
        LowerIdent "number"
        ParenClose ")"
    "#);
}

#[test]
fn multiple_patterns_with_captures() {
    let input = indoc! {r#"
    (function) @func
    (class) @cls
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "function"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "func"
      Node
        ParenOpen "("
        LowerIdent "class"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "cls"
    "#);
}
