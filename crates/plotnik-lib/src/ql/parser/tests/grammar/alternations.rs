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
            LowerIdent "identifier"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "string"
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
              LowerIdent "identifier"
              ParenClose ")"
            Tree
              ParenOpen "("
              LowerIdent "string"
              ParenClose ")"
            BracketClose "]"
          CaptureName "@value"
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
          LowerIdent "expr"
          Alt
            BracketOpen "["
            Tree
              ParenOpen "("
              LowerIdent "binary"
              ParenClose ")"
            Tree
              ParenOpen "("
              LowerIdent "unary"
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
          LowerIdent "call"
          Field
            LowerIdent "arguments"
            Colon ":"
            Alt
              BracketOpen "["
              Tree
                ParenOpen "("
                LowerIdent "string"
                ParenClose ")"
              Tree
                ParenOpen "("
                LowerIdent "number"
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
            LowerIdent "identifier"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "number"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "string"
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
          Tree
            ParenOpen "("
            UpperIdent "Expr"
            ParenClose ")"
          Tree
            ParenOpen "("
            UpperIdent "Statement"
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
            UpperIdent "Ident"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "identifier"
              ParenClose ")"
          Branch
            UpperIdent "Num"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "number"
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
            UpperIdent "A"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "a"
              ParenClose ")"
          Branch
            UpperIdent "B"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "b"
              ParenClose ")"
          Branch
            UpperIdent "C"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "c"
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
              UpperIdent "Assign"
              Colon ":"
              Tree
                ParenOpen "("
                LowerIdent "assignment_expression"
                Field
                  LowerIdent "left"
                  Colon ":"
                  Capture
                    Tree
                      ParenOpen "("
                      LowerIdent "identifier"
                      ParenClose ")"
                    CaptureName "@left"
                ParenClose ")"
            Branch
              UpperIdent "Call"
              Colon ":"
              Tree
                ParenOpen "("
                LowerIdent "call_expression"
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
            BracketClose "]"
          CaptureName "@stmt"
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
              UpperIdent "Base"
              Colon ":"
              Capture
                Tree
                  ParenOpen "("
                  LowerIdent "identifier"
                  ParenClose ")"
                CaptureName "@name"
            Branch
              UpperIdent "Access"
              Colon ":"
              Tree
                ParenOpen "("
                LowerIdent "member_expression"
                Field
                  LowerIdent "object"
                  Colon ":"
                  Capture
                    Tree
                      ParenOpen "("
                      Underscore "_"
                      ParenClose ")"
                    CaptureName "@obj"
                ParenClose ")"
            BracketClose "]"
          CaptureName "@chain"
          Type
            DoubleColon "::"
            UpperIdent "MemberChain"
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
          LowerIdent "expr"
          Alt
            BracketOpen "["
            Branch
              UpperIdent "Binary"
              Colon ":"
              Tree
                ParenOpen "("
                LowerIdent "binary_expression"
                ParenClose ")"
            Branch
              UpperIdent "Unary"
              Colon ":"
              Tree
                ParenOpen "("
                LowerIdent "unary_expression"
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
        UpperIdent "Statement"
        Equals "="
        Alt
          BracketOpen "["
          Branch
            UpperIdent "Assign"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "assignment_expression"
              ParenClose ")"
          Branch
            UpperIdent "Call"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "call_expression"
              ParenClose ")"
          Branch
            UpperIdent "Return"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "return_statement"
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
            UpperIdent "Single"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "statement"
              ParenClose ")"
          Branch
            UpperIdent "Multiple"
            Colon ":"
            Quantifier
              Tree
                ParenOpen "("
                LowerIdent "statement"
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
            UpperIdent "Pair"
            Colon ":"
            Seq
              BraceOpen "{"
              Tree
                ParenOpen "("
                LowerIdent "key"
                ParenClose ")"
              Tree
                ParenOpen "("
                LowerIdent "value"
                ParenClose ")"
              BraceClose "}"
          Branch
            UpperIdent "Single"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "value"
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
            UpperIdent "Tagged"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "a"
              ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
          Branch
            UpperIdent "Another"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "c"
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
            UpperIdent "Literal"
            Colon ":"
            Alt
              BracketOpen "["
              Tree
                ParenOpen "("
                LowerIdent "number"
                ParenClose ")"
              Tree
                ParenOpen "("
                LowerIdent "string"
                ParenClose ")"
              BracketClose "]"
          Branch
            UpperIdent "Ident"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "identifier"
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
        UpperIdent "Expression"
        Equals "="
        Alt
          BracketOpen "["
          Branch
            UpperIdent "Ident"
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
          Branch
            UpperIdent "Num"
            Colon ":"
            Capture
              Tree
                ParenOpen "("
                LowerIdent "number"
                ParenClose ")"
              CaptureName "@value"
              Type
                DoubleColon "::"
                LowerIdent "string"
          Branch
            UpperIdent "Str"
            Colon ":"
            Capture
              Tree
                ParenOpen "("
                LowerIdent "string"
                ParenClose ")"
              CaptureName "@value"
              Type
                DoubleColon "::"
                LowerIdent "string"
          Branch
            UpperIdent "Binary"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "binary_expression"
              Field
                LowerIdent "left"
                Colon ":"
                Capture
                  Tree
                    ParenOpen "("
                    UpperIdent "Expression"
                    ParenClose ")"
                  CaptureName "@left"
              Field
                LowerIdent "right"
                Colon ":"
                Capture
                  Tree
                    ParenOpen "("
                    UpperIdent "Expression"
                    ParenClose ")"
                  CaptureName "@right"
              ParenClose ")"
          BracketClose "]"
    "#);
}
