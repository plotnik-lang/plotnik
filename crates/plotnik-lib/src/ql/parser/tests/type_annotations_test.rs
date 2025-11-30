use super::helpers_test::*;
use indoc::indoc;

#[test]
fn capture_with_type_annotation() {
    let input = indoc! {r#"
    (identifier) @name :: string
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
        Type
          DoubleColon "::"
          LowerIdent "string"
    "#);
}

#[test]
fn capture_with_custom_type() {
    let input = indoc! {r#"
    (function_declaration) @fn :: FunctionDecl
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "function_declaration"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "fn"
        Type
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
fn multiple_captures_with_types() {
    let input = indoc! {r#"
    (binary
        left: (_) @left :: Node
        right: (_) @right :: string) @expr :: BinaryExpr
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
          Type
            DoubleColon "::"
            UpperIdent "Node"
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
          Type
            DoubleColon "::"
            LowerIdent "string"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "expr"
        Type
          DoubleColon "::"
          UpperIdent "BinaryExpr"
    "#);
}

#[test]
fn capture_type_missing_type_name() {
    let input = indoc! {r#"
    (identifier) @name ::
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
        Type
          DoubleColon "::"
    ---
    error: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | (identifier) @name ::
      |                      ^
    "#);
}

#[test]
fn sequence_capture_with_type() {
    let input = indoc! {r#"
    {(a) (b)} @seq :: MySequence
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Seq
        BraceOpen "{"
        Node
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
        BraceClose "}"
      Capture
        At "@"
        LowerIdent "seq"
        Type
          DoubleColon "::"
          UpperIdent "MySequence"
    "#);
}

#[test]
fn alternation_capture_with_type() {
    let input = indoc! {r#"
    [(identifier) (number)] @value :: Value
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
          LowerIdent "number"
          ParenClose ")"
        BracketClose "]"
      Capture
        At "@"
        LowerIdent "value"
        Type
          DoubleColon "::"
          UpperIdent "Value"
    "#);
}

#[test]
fn quantified_capture_with_type() {
    let input = indoc! {r#"
    (statement)+ @stmts :: Statement
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Quantifier
        Node
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        Plus "+"
      Capture
        At "@"
        LowerIdent "stmts"
        Type
          DoubleColon "::"
          UpperIdent "Statement"
    "#);
}

#[test]
fn nested_captures_with_types() {
    let input = indoc! {r#"
    (function
        name: (identifier) @name :: string
        body: (block
            (statement)* @body_stmts :: Statement)) @func :: Function
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "function"
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
          Type
            DoubleColon "::"
            LowerIdent "string"
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
            Capture
              At "@"
              LowerIdent "body_stmts"
              Type
                DoubleColon "::"
                UpperIdent "Statement"
            ParenClose ")"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "func"
        Type
          DoubleColon "::"
          UpperIdent "Function"
    "#);
}

#[test]
fn type_annotation_invalid_token_after() {
    let input = indoc! {r#"
    (identifier) @name :: (
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
        Type
          DoubleColon "::"
      Node
        ParenOpen "("
    ---
    error: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | (identifier) @name :: (
      |                       ^
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | (identifier) @name :: (
      |                        ^
    "#);
}

#[test]
fn capture_with_type_no_spaces() {
    // Parser should accept both spaced and non-spaced forms
    let input = indoc! {r#"
    (identifier) @name::string
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
        Type
          DoubleColon "::"
          LowerIdent "string"
    "#);
}
