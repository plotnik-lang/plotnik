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
      NamedNode
        ParenOpen "("
        LowerIdent "function_definition"
        Field
          LowerIdent "name"
          Colon ":"
          NamedNode
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
            NamedNode
              ParenOpen "("
              LowerIdent "parameters"
              ParenClose ")"
            Question "?"
        Field
          LowerIdent "body"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "block"
            Quantifier
              NamedNode
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
      NamedNode
        ParenOpen "("
        LowerIdent "block"
        Anchor
          Dot "."
        NamedNode
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
      NamedNode
        ParenOpen "("
        LowerIdent "if"
        Field
          LowerIdent "condition"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "expr"
            ParenClose ")"
        NamedNode
          ParenOpen "("
          LowerIdent "then_block"
          ParenClose ")"
        Field
          LowerIdent "else"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "else_block"
            ParenClose ")"
        ParenClose ")"
    "#);
}
