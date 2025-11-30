use crate::ql::parser::tests::helpers::*;
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
fn mixed_children_and_fields() {
    let input = indoc! {r#"
    (if
        condition: (expr)
        (then_block)
        else: (else_block))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "if"
        Field
          LowerIdent "condition"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "expr"
            ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "then_block"
          ParenClose ")"
        Field
          LowerIdent "else"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "else_block"
            ParenClose ")"
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