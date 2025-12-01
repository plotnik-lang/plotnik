use crate::Query;
use indoc::indoc;

// ============================================================================
// Basic Captures
// ============================================================================

#[test]
fn capture() {
    let input = indoc! {r#"
    (identifier) @name
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          CaptureName "@name"
    "#);
}

#[test]
fn capture_nested() {
    let input = indoc! {r#"
    (call function: (identifier) @func)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
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
              CaptureName "@func"
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
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
                CaptureName "@left"
            Field
              LowerIdent "right"
              Colon ":"
              Capture
                Tree
                  ParenOpen "("
                  Underscore "_"
                  ParenClose ")"
                CaptureName "@right"
            ParenClose ")"
          CaptureName "@expr"
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          CaptureName "@name"
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "function_declaration"
            ParenClose ")"
          CaptureName "@fn"
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          CaptureName "@name"
    "#);
}

#[test]
fn multiple_captures_with_types() {
    let input = indoc! {r#"
    (binary
        left: (_) @left :: Node
        right: (_) @right :: string) @expr :: BinaryExpr
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
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
                CaptureName "@left"
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
                CaptureName "@right"
                Type
                  DoubleColon "::"
                  LowerIdent "string"
            ParenClose ")"
          CaptureName "@expr"
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
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
          CaptureName "@seq"
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
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
          CaptureName "@value"
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Quantifier
            Tree
              ParenOpen "("
              LowerIdent "statement"
              ParenClose ")"
            Plus "+"
          CaptureName "@stmts"
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
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
                CaptureName "@name"
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
                  CaptureName "@body_stmts"
                  Type
                    DoubleColon "::"
                    UpperIdent "Statement"
                ParenClose ")"
            ParenClose ")"
          CaptureName "@func"
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

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          CaptureName "@name"
          Type
            DoubleColon "::"
            LowerIdent "string"
    "#);
}
