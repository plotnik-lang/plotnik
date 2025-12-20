use crate::Query;
use indoc::indoc;

#[test]
fn capture() {
    let input = indoc! {r#"
    Q = (identifier) @name
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = (call function: (identifier) @func)
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          Id "call"
          Capture
            Field
              Id "function"
              Colon ":"
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
    Q = (binary
        left: (_) @left
        right: (_) @right) @expr
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Capture
          Tree
            ParenOpen "("
            Id "binary"
            Capture
              Field
                Id "left"
                Colon ":"
                Tree
                  ParenOpen "("
                  Underscore "_"
                  ParenClose ")"
              At "@"
              Id "left"
            Capture
              Field
                Id "right"
                Colon ":"
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

#[test]
fn capture_with_type_annotation() {
    let input = indoc! {r#"
    Q = (identifier) @name :: string
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = (function_declaration) @fn :: FunctionDecl
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = (identifier) @name
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = (binary
        left: (_) @left :: Node
        right: (_) @right :: string) @expr :: BinaryExpr
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Capture
          Tree
            ParenOpen "("
            Id "binary"
            Capture
              Field
                Id "left"
                Colon ":"
                Tree
                  ParenOpen "("
                  Underscore "_"
                  ParenClose ")"
              At "@"
              Id "left"
              Type
                DoubleColon "::"
                Id "Node"
            Capture
              Field
                Id "right"
                Colon ":"
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
    Q = {(a) (b)} @seq :: MySequence
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = [(identifier) (number)] @value :: Value
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Capture
          Alt
            BracketOpen "["
            Branch
              Tree
                ParenOpen "("
                Id "identifier"
                ParenClose ")"
            Branch
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
    Q = (statement)+ @stmts :: Statement
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = (function
        name: (identifier) @name :: string
        body: (block
            (statement)* @body_stmts :: Statement)) @func :: Function
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Capture
          Tree
            ParenOpen "("
            Id "function"
            Capture
              Field
                Id "name"
                Colon ":"
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
    Q = (identifier) @name::string
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
fn capture_literal() {
    let input = indoc! {r#"
    Q = "foo" @keyword
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Capture
          Str
            DoubleQuote "\""
            StrVal "foo"
            DoubleQuote "\""
          At "@"
          Id "keyword"
    "#);
}

#[test]
fn capture_literal_with_type() {
    let input = indoc! {r#"
    Q = "return" @kw :: string
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Capture
          Str
            DoubleQuote "\""
            StrVal "return"
            DoubleQuote "\""
          At "@"
          Id "kw"
          Type
            DoubleColon "::"
            Id "string"
    "#);
}

#[test]
fn capture_literal_in_tree() {
    let input = indoc! {r#"
    Q = (binary_expression "+" @op)
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          Id "binary_expression"
          Capture
            Str
              DoubleQuote "\""
              StrVal "+"
              DoubleQuote "\""
            At "@"
            Id "op"
          ParenClose ")"
    "#);
}

#[test]
fn capture_literal_with_quantifier() {
    let input = indoc! {r#"
    Q = ","* @commas
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Capture
          Quantifier
            Str
              DoubleQuote "\""
              StrVal ","
              DoubleQuote "\""
            Star "*"
          At "@"
          Id "commas"
    "#);
}
