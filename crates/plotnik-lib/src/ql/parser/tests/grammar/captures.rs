use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

// ============================================================================
// Basic Captures
// ============================================================================

#[test]
fn capture() {
    let input = indoc! {r#"
    (identifier) @name
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Capture
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
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
      Tree
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "function"
          Colon ":"
          Capture
            Tree
              ParenOpen "("
              LowerIdent "identifier"
              ParenClose ")"
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
      Capture
        Tree
          ParenOpen "("
          LowerIdent "binary"
          Field
            LowerIdent "left"
            Colon ":"
            Capture
              Tree
                ParenOpen "("
                Underscore "_"
                ParenClose ")"
              At "@"
              LowerIdent "left"
          Field
            LowerIdent "right"
            Colon ":"
            Capture
              Tree
                ParenOpen "("
                Underscore "_"
                ParenClose ")"
              At "@"
              LowerIdent "right"
          ParenClose ")"
        At "@"
        LowerIdent "expr"
    "#);
}

// ============================================================================
// Type Annotations
// ============================================================================

#[test]
fn capture_with_type_annotation() {
    let input = indoc! {r#"
    (identifier) @name :: string
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Capture
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
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
      Capture
        Tree
          ParenOpen "("
          LowerIdent "function_declaration"
          ParenClose ")"
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
      Capture
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
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
      Capture
        Tree
          ParenOpen "("
          LowerIdent "binary"
          Field
            LowerIdent "left"
            Colon ":"
            Capture
              Tree
                ParenOpen "("
                Underscore "_"
                ParenClose ")"
              At "@"
              LowerIdent "left"
              Type
                DoubleColon "::"
                UpperIdent "Node"
          Field
            LowerIdent "right"
            Colon ":"
            Capture
              Tree
                ParenOpen "("
                Underscore "_"
                ParenClose ")"
              At "@"
              LowerIdent "right"
              Type
                DoubleColon "::"
                LowerIdent "string"
          ParenClose ")"
        At "@"
        LowerIdent "expr"
        Type
          DoubleColon "::"
          UpperIdent "BinaryExpr"
    "#);
}

#[test]
fn sequence_capture_with_type() {
    let input = indoc! {r#"
    {(a) (b)} @seq :: MySequence
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Capture
        Seq
          BraceOpen "{"
          Tree
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
          BraceClose "}"
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
      Capture
        Alt
          BracketOpen "["
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "number"
            ParenClose ")"
          BracketClose "]"
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
      Capture
        Quantifier
          Tree
            ParenOpen "("
            LowerIdent "statement"
            ParenClose ")"
          Plus "+"
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
      Capture
        Tree
          ParenOpen "("
          LowerIdent "function"
          Field
            LowerIdent "name"
            Colon ":"
            Capture
              Tree
                ParenOpen "("
                LowerIdent "identifier"
                ParenClose ")"
              At "@"
              LowerIdent "name"
              Type
                DoubleColon "::"
                LowerIdent "string"
          Field
            LowerIdent "body"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "block"
              Capture
                Quantifier
                  Tree
                    ParenOpen "("
                    LowerIdent "statement"
                    ParenClose ")"
                  Star "*"
                At "@"
                LowerIdent "body_stmts"
                Type
                  DoubleColon "::"
                  UpperIdent "Statement"
              ParenClose ")"
          ParenClose ")"
        At "@"
        LowerIdent "func"
        Type
          DoubleColon "::"
          UpperIdent "Function"
    "#);
}

#[test]
fn capture_with_type_no_spaces() {
    let input = indoc! {r#"
    (identifier) @name::string
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Capture
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        At "@"
        LowerIdent "name"
        Type
          DoubleColon "::"
          LowerIdent "string"
    "#);
}
