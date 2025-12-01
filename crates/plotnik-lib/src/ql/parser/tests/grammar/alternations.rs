use crate::Query;
use indoc::indoc;

// ============================================================================
// Unlabeled Alternations
// ============================================================================

#[test]
fn alternation() {
    let input = indoc! {r#"
    [(identifier) (string)]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Tree
            ParenOpen "("
            Id "identifier"
            ParenClose ")"
          Tree
            ParenOpen "("
            Id "string"
            ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn alternation_with_anonymous() {
    let input = indoc! {r#"
    ["true" "false"]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Lit
            StringLit "\"true\""
          Lit
            StringLit "\"false\""
          BracketClose "]"
    "#);
}

#[test]
fn alternation_with_capture() {
    let input = indoc! {r#"
    [(identifier) (string)] @value
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
              Id "string"
              ParenClose ")"
            BracketClose "]"
          At "@"
          Id "value"
    "#);
}

#[test]
fn alternation_nested() {
    let input = indoc! {r#"
    (expr
        [(binary) (unary)])
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "expr"
          Alt
            BracketOpen "["
            Tree
              ParenOpen "("
              Id "binary"
              ParenClose ")"
            Tree
              ParenOpen "("
              Id "unary"
              ParenClose ")"
            BracketClose "]"
          ParenClose ")"
    "#);
}

#[test]
fn alternation_in_field() {
    let input = indoc! {r#"
    (call
        arguments: [(string) (number)])
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "call"
          Field
            Id "arguments"
            Colon ":"
            Alt
              BracketOpen "["
              Tree
                ParenOpen "("
                Id "string"
                ParenClose ")"
              Tree
                ParenOpen "("
                Id "number"
                ParenClose ")"
              BracketClose "]"
          ParenClose ")"
    "#);
}

#[test]
fn unlabeled_alternation_three_items() {
    let input = indoc! {r#"
    [(identifier) (number) (string)]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
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
          Tree
            ParenOpen "("
            Id "string"
            ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn upper_ident_in_alternation_not_followed_by_colon() {
    let input = indoc! {r#"
    [(Expr) (Statement)]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Ref
            ParenOpen "("
            Id "Expr"
            ParenClose ")"
          Ref
            ParenOpen "("
            Id "Statement"
            ParenClose ")"
          BracketClose "]"
    ---
    error: undefined reference: `Expr`
      |
    1 | [(Expr) (Statement)]
      |   ^^^^ undefined reference: `Expr`
    error: undefined reference: `Statement`
      |
    1 | [(Expr) (Statement)]
      |          ^^^^^^^^^ undefined reference: `Statement`
    "#);
}

// ============================================================================
// Tagged Alternations
// ============================================================================

#[test]
fn tagged_alternation_simple() {
    let input = indoc! {r#"
    [
        Ident: (identifier)
        Num: (number)
    ]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Branch
            Id "Ident"
            Colon ":"
            Tree
              ParenOpen "("
              Id "identifier"
              ParenClose ")"
          Branch
            Id "Num"
            Colon ":"
            Tree
              ParenOpen "("
              Id "number"
              ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn tagged_alternation_single_line() {
    let input = indoc! {r#"
    [A: (a) B: (b) C: (c)]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Branch
            Id "A"
            Colon ":"
            Tree
              ParenOpen "("
              Id "a"
              ParenClose ")"
          Branch
            Id "B"
            Colon ":"
            Tree
              ParenOpen "("
              Id "b"
              ParenClose ")"
          Branch
            Id "C"
            Colon ":"
            Tree
              ParenOpen "("
              Id "c"
              ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn tagged_alternation_with_captures() {
    let input = indoc! {r#"
    [
        Assign: (assignment_expression left: (identifier) @left)
        Call: (call_expression function: (identifier) @func)
    ] @stmt
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Alt
            BracketOpen "["
            Branch
              Id "Assign"
              Colon ":"
              Tree
                ParenOpen "("
                Id "assignment_expression"
                Field
                  Id "left"
                  Colon ":"
                  Capture
                    Tree
                      ParenOpen "("
                      Id "identifier"
                      ParenClose ")"
                    At "@"
                    Id "left"
                ParenClose ")"
            Branch
              Id "Call"
              Colon ":"
              Tree
                ParenOpen "("
                Id "call_expression"
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
            BracketClose "]"
          At "@"
          Id "stmt"
    "#);
}

#[test]
fn tagged_alternation_with_type_annotation() {
    let input = indoc! {r#"
    [
        Base: (identifier) @name
        Access: (member_expression object: (_) @obj)
    ] @chain :: MemberChain
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Alt
            BracketOpen "["
            Branch
              Id "Base"
              Colon ":"
              Capture
                Tree
                  ParenOpen "("
                  Id "identifier"
                  ParenClose ")"
                At "@"
                Id "name"
            Branch
              Id "Access"
              Colon ":"
              Tree
                ParenOpen "("
                Id "member_expression"
                Field
                  Id "object"
                  Colon ":"
                  Capture
                    Tree
                      ParenOpen "("
                      Underscore "_"
                      ParenClose ")"
                    At "@"
                    Id "obj"
                ParenClose ")"
            BracketClose "]"
          At "@"
          Id "chain"
          Type
            DoubleColon "::"
            Id "MemberChain"
    "#);
}

#[test]
fn tagged_alternation_nested() {
    let input = indoc! {r#"
    (expr
        [
            Binary: (binary_expression)
            Unary: (unary_expression)
        ])
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "expr"
          Alt
            BracketOpen "["
            Branch
              Id "Binary"
              Colon ":"
              Tree
                ParenOpen "("
                Id "binary_expression"
                ParenClose ")"
            Branch
              Id "Unary"
              Colon ":"
              Tree
                ParenOpen "("
                Id "unary_expression"
                ParenClose ")"
            BracketClose "]"
          ParenClose ")"
    "#);
}

#[test]
fn tagged_alternation_in_named_def() {
    let input = indoc! {r#"
    Statement = [
        Assign: (assignment_expression)
        Call: (call_expression)
        Return: (return_statement)
    ]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Id "Statement"
        Equals "="
        Alt
          BracketOpen "["
          Branch
            Id "Assign"
            Colon ":"
            Tree
              ParenOpen "("
              Id "assignment_expression"
              ParenClose ")"
          Branch
            Id "Call"
            Colon ":"
            Tree
              ParenOpen "("
              Id "call_expression"
              ParenClose ")"
          Branch
            Id "Return"
            Colon ":"
            Tree
              ParenOpen "("
              Id "return_statement"
              ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn tagged_alternation_with_quantifier() {
    let input = indoc! {r#"
    [
        Single: (statement)
        Multiple: (statement)+
    ]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Branch
            Id "Single"
            Colon ":"
            Tree
              ParenOpen "("
              Id "statement"
              ParenClose ")"
          Branch
            Id "Multiple"
            Colon ":"
            Quantifier
              Tree
                ParenOpen "("
                Id "statement"
                ParenClose ")"
              Plus "+"
          BracketClose "]"
    "#);
}

#[test]
fn tagged_alternation_with_sequence() {
    let input = indoc! {r#"
    [
        Pair: {(key) (value)}
        Single: (value)
    ]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Branch
            Id "Pair"
            Colon ":"
            Seq
              BraceOpen "{"
              Tree
                ParenOpen "("
                Id "key"
                ParenClose ")"
              Tree
                ParenOpen "("
                Id "value"
                ParenClose ")"
              BraceClose "}"
          Branch
            Id "Single"
            Colon ":"
            Tree
              ParenOpen "("
              Id "value"
              ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn mixed_tagged_and_untagged() {
    let input = indoc! {r#"
    [Tagged: (a) (b) Another: (c)]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Branch
            Id "Tagged"
            Colon ":"
            Tree
              ParenOpen "("
              Id "a"
              ParenClose ")"
          Tree
            ParenOpen "("
            Id "b"
            ParenClose ")"
          Branch
            Id "Another"
            Colon ":"
            Tree
              ParenOpen "("
              Id "c"
              ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn tagged_alternation_with_nested_alternation() {
    let input = indoc! {r#"
    [
        Literal: [(number) (string)]
        Ident: (identifier)
    ]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Branch
            Id "Literal"
            Colon ":"
            Alt
              BracketOpen "["
              Tree
                ParenOpen "("
                Id "number"
                ParenClose ")"
              Tree
                ParenOpen "("
                Id "string"
                ParenClose ")"
              BracketClose "]"
          Branch
            Id "Ident"
            Colon ":"
            Tree
              ParenOpen "("
              Id "identifier"
              ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn tagged_alternation_full_example() {
    let input = indoc! {r#"
    Expression = [
        Ident: (identifier) @name :: string
        Num: (number) @value :: string
        Str: (string) @value :: string
        Binary: (binary_expression
            left: (Expression) @left
            right: (Expression) @right)
    ]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Id "Expression"
        Equals "="
        Alt
          BracketOpen "["
          Branch
            Id "Ident"
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
          Branch
            Id "Num"
            Colon ":"
            Capture
              Tree
                ParenOpen "("
                Id "number"
                ParenClose ")"
              At "@"
              Id "value"
              Type
                DoubleColon "::"
                Id "string"
          Branch
            Id "Str"
            Colon ":"
            Capture
              Tree
                ParenOpen "("
                Id "string"
                ParenClose ")"
              At "@"
              Id "value"
              Type
                DoubleColon "::"
                Id "string"
          Branch
            Id "Binary"
            Colon ":"
            Tree
              ParenOpen "("
              Id "binary_expression"
              Field
                Id "left"
                Colon ":"
                Capture
                  Ref
                    ParenOpen "("
                    Id "Expression"
                    ParenClose ")"
                  At "@"
                  Id "left"
              Field
                Id "right"
                Colon ":"
                Capture
                  Ref
                    ParenOpen "("
                    Id "Expression"
                    ParenClose ")"
                  At "@"
                  Id "right"
              ParenClose ")"
          BracketClose "]"
    "#);
}
