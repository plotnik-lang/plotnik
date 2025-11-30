use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

#[test]
fn field_pattern() {
    let input = indoc! {r#"
    (call function: (identifier))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Tree
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "function"
          Colon ":"
          Tree
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
      Tree
        ParenOpen "("
        LowerIdent "assignment"
        Field
          LowerIdent "left"
          Colon ":"
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        Field
          LowerIdent "right"
          Colon ":"
          Tree
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
      Tree
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
      Tree
        ParenOpen "("
        LowerIdent "function"
        NegatedField
          Negation "!"
          LowerIdent "async"
        Field
          LowerIdent "name"
          Colon ":"
          Tree
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
      Tree
        ParenOpen "("
        LowerIdent "if"
        Field
          LowerIdent "condition"
          Colon ":"
          Tree
            ParenOpen "("
            LowerIdent "expr"
            ParenClose ")"
        Tree
          ParenOpen "("
          LowerIdent "then_block"
          ParenClose ")"
        Field
          LowerIdent "else"
          Colon ":"
          Tree
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
      Capture
        Tree
          ParenOpen "("
          LowerIdent "function"
          ParenClose ")"
        At "@"
        LowerIdent "func"
      Capture
        Tree
          ParenOpen "("
          LowerIdent "class"
          ParenClose ")"
        At "@"
        LowerIdent "cls"
    "#);
}
