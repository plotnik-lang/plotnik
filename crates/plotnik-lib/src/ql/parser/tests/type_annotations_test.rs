use super::helpers_test::*;
use indoc::indoc;

#[test]
fn capture_with_type_annotation() {
    let input = indoc! {r#"
    (identifier) @name::string
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "name"
        TypeAnnotation
          DoubleColon "::"
          LowerIdent "string"
    "#);
}

#[test]
fn capture_with_custom_type() {
    let input = indoc! {r#"
    (function_declaration) @fn::FunctionDecl
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "function_declaration"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "fn"
        TypeAnnotation
          DoubleColon "::"
          UpperIdent "FunctionDecl"
    "#);
}

#[test]
fn capture_without_type_annotation() {
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
        LowerIdent "name"
    "#);
}

#[test]
fn multiple_captures_with_types() {
    let input = indoc! {r#"
    (binary
        left: (_) @left::Node
        right: (_) @right::string) @expr::BinaryExpr
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
          LowerIdent "left"
          TypeAnnotation
            DoubleColon "::"
            UpperIdent "Node"
        Field
          LowerIdent "right"
          Colon ":"
          NamedNode
            ParenOpen "("
            Underscore "_"
            ParenClose ")"
        Capture
          At "@"
          LowerIdent "right"
          TypeAnnotation
            DoubleColon "::"
            LowerIdent "string"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "expr"
        TypeAnnotation
          DoubleColon "::"
          UpperIdent "BinaryExpr"
    "#);
}

#[test]
fn capture_type_missing_type_name() {
    let input = indoc! {r#"
    (identifier) @name::
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "name"
        TypeAnnotation
          DoubleColon "::"
    ---
    error: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | (identifier) @name::
      |                     ^
    "#);
}

#[test]
fn sequence_capture_with_type() {
    let input = indoc! {r#"
    {(a) (b)} @seq::MySequence
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Sequence
        BraceOpen "{"
        NamedNode
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        NamedNode
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
        BraceClose "}"
      Capture
        At "@"
        LowerIdent "seq"
        TypeAnnotation
          DoubleColon "::"
          UpperIdent "MySequence"
    "#);
}

#[test]
fn alternation_capture_with_type() {
    let input = indoc! {r#"
    [(identifier) (number)] @value::Value
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
          LowerIdent "number"
          ParenClose ")"
        BracketClose "]"
      Capture
        At "@"
        LowerIdent "value"
        TypeAnnotation
          DoubleColon "::"
          UpperIdent "Value"
    "#);
}

#[test]
fn quantified_capture_with_type() {
    let input = indoc! {r#"
    (statement)+ @stmts::Statement
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Quantifier
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        Plus "+"
      Capture
        At "@"
        LowerIdent "stmts"
        TypeAnnotation
          DoubleColon "::"
          UpperIdent "Statement"
    "#);
}

#[test]
fn nested_captures_with_types() {
    let input = indoc! {r#"
    (function
        name: (identifier) @name::string
        body: (block
            (statement)* @body_stmts::Statement)) @func::Function
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "function"
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
          TypeAnnotation
            DoubleColon "::"
            LowerIdent "string"
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
            Capture
              At "@"
              LowerIdent "body_stmts"
              TypeAnnotation
                DoubleColon "::"
                UpperIdent "Statement"
            ParenClose ")"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "func"
        TypeAnnotation
          DoubleColon "::"
          UpperIdent "Function"
    "#);
}

#[test]
fn type_annotation_invalid_token_after() {
    let input = indoc! {r#"
    (identifier) @name::(
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "name"
        TypeAnnotation
          DoubleColon "::"
      NamedNode
        ParenOpen "("
    ---
    error: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | (identifier) @name::(
      |                     ^
    error: expected ')'
      |
    1 | (identifier) @name::(
      |                      ^
    "#);
}
