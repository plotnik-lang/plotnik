use super::helpers_test::*;
use indoc::indoc;

#[test]
fn complex_function_query() {
    let input = indoc! {r#"
    (function_definition
        name: (identifier) @name
        parameters: (parameters)?
        body: (block (statement)*))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "function_definition"
        Field
          LowerIdent "name"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        Capture
          At "@"
          LowerIdent "name"
        Field
          LowerIdent "parameters"
          Colon ":"
          Quantifier
            Node
              ParenOpen "("
              LowerIdent "parameters"
              ParenClose ")"
            Question "?"
        Field
          LowerIdent "body"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "block"
            Quantifier
              Node
                ParenOpen "("
                LowerIdent "statement"
                ParenClose ")"
              Star "*"
            ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn anchor_dot() {
    let input = indoc! {r#"
    (block
        . (first_statement))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "block"
        Anchor
          Dot "."
        Node
          ParenOpen "("
          LowerIdent "first_statement"
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
