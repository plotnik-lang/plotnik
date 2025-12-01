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
            Id "identifier"
            ParenClose ")"
          At "@"
          Id "name"
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
          Id "call"
          Field
            Id "function"
            Colon ":"
            Capture
              Tree
                ParenOpen "("
                Id "identifier"
                ParenClose ")"
              At "@"
              Id "func"
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
            Id "binary"
            Field
              Id "left"
              Colon ":"
              Capture
                Tree
                  ParenOpen "("
                  Underscore "_"
                  ParenClose ")"
                At "@"
                Id "left"
            Field
              Id "right"
              Colon ":"
              Capture
                Tree
                  ParenOpen "("
                  Underscore "_"
                  ParenClose ")"
                At "@"
                Id "right"
            ParenClose ")"
          At "@"
          Id "expr"
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
            Id "identifier"
            ParenClose ")"
          At "@"
          Id "name"
          Type
            DoubleColon "::"
            Id "string"
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
            Id "function_declaration"
            ParenClose ")"
          At "@"
          Id "fn"
          Type
            DoubleColon "::"
            Id "FunctionDecl"
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
            Id "identifier"
            ParenClose ")"
          At "@"
          Id "name"
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
            Id "binary"
            Field
              Id "left"
              Colon ":"
              Capture
                Tree
                  ParenOpen "("
                  Underscore "_"
                  ParenClose ")"
                At "@"
                Id "left"
                Type
                  DoubleColon "::"
                  Id "Node"
            Field
              Id "right"
              Colon ":"
              Capture
                Tree
                  ParenOpen "("
                  Underscore "_"
                  ParenClose ")"
                At "@"
                Id "right"
                Type
                  DoubleColon "::"
                  Id "string"
            ParenClose ")"
          At "@"
          Id "expr"
          Type
            DoubleColon "::"
            Id "BinaryExpr"
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
              Id "a"
              ParenClose ")"
            Tree
              ParenOpen "("
              Id "b"
              ParenClose ")"
            BraceClose "}"
          At "@"
          Id "seq"
          Type
            DoubleColon "::"
            Id "MySequence"
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
              Id "identifier"
              ParenClose ")"
            Tree
              ParenOpen "("
              Id "number"
              ParenClose ")"
            BracketClose "]"
          At "@"
          Id "value"
          Type
            DoubleColon "::"
            Id "Value"
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
              Id "statement"
              ParenClose ")"
            Plus "+"
          At "@"
          Id "stmts"
          Type
            DoubleColon "::"
            Id "Statement"
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
            Id "function"
            Field
              Id "name"
              Colon ":"
              Capture
                Tree
                  ParenOpen "("
                  Id "identifier"
                  ParenClose ")"
                At "@"
                Id "name"
                Type
                  DoubleColon "::"
                  Id "string"
            Field
              Id "body"
              Colon ":"
              Tree
                ParenOpen "("
                Id "block"
                Capture
                  Quantifier
                    Tree
                      ParenOpen "("
                      Id "statement"
                      ParenClose ")"
                    Star "*"
                  At "@"
                  Id "body_stmts"
                  Type
                    DoubleColon "::"
                    Id "Statement"
                ParenClose ")"
            ParenClose ")"
          At "@"
          Id "func"
          Type
            DoubleColon "::"
            Id "Function"
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
            Id "identifier"
            ParenClose ")"
          At "@"
          Id "name"
          Type
            DoubleColon "::"
            Id "string"
    "#);
}
