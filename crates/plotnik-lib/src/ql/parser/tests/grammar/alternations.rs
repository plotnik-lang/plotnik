use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

// ============================================================================
// Unlabeled Alternations
// ============================================================================

#[test]
fn alternation() {
    let input = indoc! {r#"
    [(identifier) (string)]
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
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
          LowerIdent "string"
          ParenClose ")"
        BracketClose "]"
      Capture
        At "@"
        LowerIdent "value"
    "#);
}

#[test]
fn alternation_nested() {
    let input = indoc! {r#"
    (expr
        [(binary) (unary)])
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "expr"
        Alt
          BracketOpen "["
          Node
            ParenOpen "("
            LowerIdent "binary"
            ParenClose ")"
          Node
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "arguments"
          Colon ":"
          Alt
            BracketOpen "["
            Node
              ParenOpen "("
              LowerIdent "string"
              ParenClose ")"
            Node
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
        Node
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Node
          ParenOpen "("
          UpperIdent "Expr"
          ParenClose ")"
        Node
          ParenOpen "("
          UpperIdent "Statement"
          ParenClose ")"
        BracketClose "]"
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Branch
          UpperIdent "Ident"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        Branch
          UpperIdent "Num"
          Colon ":"
          Node
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Branch
          UpperIdent "A"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
        Branch
          UpperIdent "B"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
        Branch
          UpperIdent "C"
          Colon ":"
          Node
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Branch
          UpperIdent "Assign"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "assignment_expression"
            Field
              LowerIdent "left"
              Colon ":"
              Node
                ParenOpen "("
                LowerIdent "identifier"
                ParenClose ")"
            Capture
              At "@"
              LowerIdent "left"
            ParenClose ")"
        Branch
          UpperIdent "Call"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "call_expression"
            Field
              LowerIdent "function"
              Colon ":"
              Node
                ParenOpen "("
                LowerIdent "identifier"
                ParenClose ")"
            Capture
              At "@"
              LowerIdent "func"
            ParenClose ")"
        BracketClose "]"
      Capture
        At "@"
        LowerIdent "stmt"
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Branch
          UpperIdent "Base"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        Capture
          At "@"
          LowerIdent "name"
        Branch
          UpperIdent "Access"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "member_expression"
            Field
              LowerIdent "object"
              Colon ":"
              Node
                ParenOpen "("
                Underscore "_"
                ParenClose ")"
            Capture
              At "@"
              LowerIdent "obj"
            ParenClose ")"
        BracketClose "]"
      Capture
        At "@"
        LowerIdent "chain"
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "expr"
        Alt
          BracketOpen "["
          Branch
            UpperIdent "Binary"
            Colon ":"
            Node
              ParenOpen "("
              LowerIdent "binary_expression"
              ParenClose ")"
          Branch
            UpperIdent "Unary"
            Colon ":"
            Node
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "Statement"
        Equals "="
        Alt
          BracketOpen "["
          Branch
            UpperIdent "Assign"
            Colon ":"
            Node
              ParenOpen "("
              LowerIdent "assignment_expression"
              ParenClose ")"
          Branch
            UpperIdent "Call"
            Colon ":"
            Node
              ParenOpen "("
              LowerIdent "call_expression"
              ParenClose ")"
          Branch
            UpperIdent "Return"
            Colon ":"
            Node
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Branch
          UpperIdent "Single"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "statement"
            ParenClose ")"
        Branch
          UpperIdent "Multiple"
          Colon ":"
          Quantifier
            Node
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Branch
          UpperIdent "Pair"
          Colon ":"
          Seq
            BraceOpen "{"
            Node
              ParenOpen "("
              LowerIdent "key"
              ParenClose ")"
            Node
              ParenOpen "("
              LowerIdent "value"
              ParenClose ")"
            BraceClose "}"
        Branch
          UpperIdent "Single"
          Colon ":"
          Node
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Branch
          UpperIdent "Tagged"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
        Branch
          UpperIdent "Another"
          Colon ":"
          Node
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Branch
          UpperIdent "Literal"
          Colon ":"
          Alt
            BracketOpen "["
            Node
              ParenOpen "("
              LowerIdent "number"
              ParenClose ")"
            Node
              ParenOpen "("
              LowerIdent "string"
              ParenClose ")"
            BracketClose "]"
        Branch
          UpperIdent "Ident"
          Colon ":"
          Node
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "Expression"
        Equals "="
        Alt
          BracketOpen "["
          Branch
            UpperIdent "Ident"
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
          Branch
            UpperIdent "Num"
            Colon ":"
            Node
              ParenOpen "("
              LowerIdent "number"
              ParenClose ")"
          Capture
            At "@"
            LowerIdent "value"
            Type
              DoubleColon "::"
              LowerIdent "string"
          Branch
            UpperIdent "Str"
            Colon ":"
            Node
              ParenOpen "("
              LowerIdent "string"
              ParenClose ")"
          Capture
            At "@"
            LowerIdent "value"
            Type
              DoubleColon "::"
              LowerIdent "string"
          Branch
            UpperIdent "Binary"
            Colon ":"
            Node
              ParenOpen "("
              LowerIdent "binary_expression"
              Field
                LowerIdent "left"
                Colon ":"
                Node
                  ParenOpen "("
                  UpperIdent "Expression"
                  ParenClose ")"
              Capture
                At "@"
                LowerIdent "left"
              Field
                LowerIdent "right"
                Colon ":"
                Node
                  ParenOpen "("
                  UpperIdent "Expression"
                  ParenClose ")"
              Capture
                At "@"
                LowerIdent "right"
              ParenClose ")"
          BracketClose "]"
    "#);
}