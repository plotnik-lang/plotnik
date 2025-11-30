use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

#[test]
fn simple_named_def() {
    let input = indoc! {r#"
    Expr = (identifier)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "Expr"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
    "#);
}

#[test]
fn named_def_with_alternation() {
    let input = indoc! {r#"
    Value = [(identifier) (number) (string)]
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "Value"
        Equals "="
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
fn named_def_with_sequence() {
    let input = indoc! {r#"
    Pair = {(identifier) (expression)}
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "Pair"
        Equals "="
        Seq
          BraceOpen "{"
          Node
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          Node
            ParenOpen "("
            LowerIdent "expression"
            ParenClose ")"
          BraceClose "}"
    "#);
}

#[test]
fn named_def_with_captures() {
    let input = indoc! {r#"
    BinaryOp = (binary_expression
        left: (_) @left
        operator: _ @op
        right: (_) @right)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "BinaryOp"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "binary_expression"
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
          Field
            LowerIdent "operator"
            Colon ":"
            Wildcard
              Underscore "_"
          Capture
            At "@"
            LowerIdent "op"
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
          ParenClose ")"
    "#);
}

#[test]
fn multiple_named_defs() {
    let input = indoc! {r#"
    Expr = (expression)
    Stmt = (statement)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "Expr"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "expression"
          ParenClose ")"
      Def
        UpperIdent "Stmt"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
    "#);
}

#[test]
fn named_def_then_pattern() {
    let input = indoc! {r#"
    Expr = [(identifier) (number)]
    (program (Expr) @value)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "Expr"
        Equals "="
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
      Node
        ParenOpen "("
        LowerIdent "program"
        Node
          ParenOpen "("
          UpperIdent "Expr"
          ParenClose ")"
        Capture
          At "@"
          LowerIdent "value"
        ParenClose ")"
    "#);
}

#[test]
fn named_def_referencing_another() {
    let input = indoc! {r#"
    Literal = [(number) (string)]
    Expr = [(identifier) (Literal)]
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "Literal"
        Equals "="
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
      Def
        UpperIdent "Expr"
        Equals "="
        Alt
          BracketOpen "["
          Node
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          Node
            ParenOpen "("
            UpperIdent "Literal"
            ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn named_def_with_quantifier() {
    let input = indoc! {r#"
    Statements = (statement)+
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "Statements"
        Equals "="
        Quantifier
          Node
            ParenOpen "("
            LowerIdent "statement"
            ParenClose ")"
          Plus "+"
    "#);
}

#[test]
fn named_def_complex_recursive() {
    let input = indoc! {r#"
    NestedCall = (call_expression
        function: [(identifier) @name (NestedCall) @inner]
        arguments: (arguments))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "NestedCall"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "call_expression"
          Field
            LowerIdent "function"
            Colon ":"
            Alt
              BracketOpen "["
              Node
                ParenOpen "("
                LowerIdent "identifier"
                ParenClose ")"
              Capture
                At "@"
                LowerIdent "name"
              Node
                ParenOpen "("
                UpperIdent "NestedCall"
                ParenClose ")"
              Capture
                At "@"
                LowerIdent "inner"
              BracketClose "]"
          Field
            LowerIdent "arguments"
            Colon ":"
            Node
              ParenOpen "("
              LowerIdent "arguments"
              ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn named_def_with_type_annotation() {
    let input = indoc! {r#"
    Func = (function_declaration
        name: (identifier) @name :: string
        body: (_) @body)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "Func"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "function_declaration"
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
              Underscore "_"
              ParenClose ")"
          Capture
            At "@"
            LowerIdent "body"
          ParenClose ")"
    "#);
}

#[test]
fn upper_ident_not_followed_by_equals_is_pattern() {
    let input = indoc! {r#"
    (Expr)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        UpperIdent "Expr"
        ParenClose ")"
    "#);
}

#[test]
fn bare_upper_ident_not_followed_by_equals_is_node() {
    let input = indoc! {r#"
    Expr
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        UpperIdent "Expr"
    "#);
}

#[test]
fn named_def_missing_equals() {
    let input = indoc! {r#"
    Expr (identifier)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        UpperIdent "Expr"
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
    "#);
}