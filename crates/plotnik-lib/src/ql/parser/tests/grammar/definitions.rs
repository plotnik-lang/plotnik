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
        Tree
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
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          Tree
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
        Tree
          ParenOpen "("
          LowerIdent "binary_expression"
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
            LowerIdent "operator"
            Colon ":"
            Capture
              Wildcard
                Underscore "_"
              At "@"
              LowerIdent "op"
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
        Tree
          ParenOpen "("
          LowerIdent "expression"
          ParenClose ")"
      Def
        UpperIdent "Stmt"
        Equals "="
        Tree
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
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "number"
            ParenClose ")"
          BracketClose "]"
      Def
        Tree
          ParenOpen "("
          LowerIdent "program"
          Capture
            Tree
              ParenOpen "("
              UpperIdent "Expr"
              ParenClose ")"
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
          Tree
            ParenOpen "("
            LowerIdent "number"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "string"
            ParenClose ")"
          BracketClose "]"
      Def
        UpperIdent "Expr"
        Equals "="
        Alt
          BracketOpen "["
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          Tree
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
          Tree
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
        Tree
          ParenOpen "("
          LowerIdent "call_expression"
          Field
            LowerIdent "function"
            Colon ":"
            Alt
              BracketOpen "["
              Capture
                Tree
                  ParenOpen "("
                  LowerIdent "identifier"
                  ParenClose ")"
                At "@"
                LowerIdent "name"
              Capture
                Tree
                  ParenOpen "("
                  UpperIdent "NestedCall"
                  ParenClose ")"
                At "@"
                LowerIdent "inner"
              BracketClose "]"
          Field
            LowerIdent "arguments"
            Colon ":"
            Tree
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
        Tree
          ParenOpen "("
          LowerIdent "function_declaration"
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
            Capture
              Tree
                ParenOpen "("
                Underscore "_"
                ParenClose ")"
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
      Def
        Tree
          ParenOpen "("
          UpperIdent "Expr"
          ParenClose ")"
    "#);
}

#[test]
fn bare_upper_ident_not_followed_by_equals_is_error() {
    let input = indoc! {r#"
    Expr
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Error
          UpperIdent "Expr"
    ---
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | Expr
      | ^^^^
    "#);
}

#[test]
fn named_def_missing_equals() {
    let input = indoc! {r#"
    Expr (identifier)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Error
          UpperIdent "Expr"
      Def
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
    ---
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | Expr (identifier)
      | ^^^^
    error: unnamed definition must be last in file; add a name: `Name = Expr`
      |
    1 | Expr (identifier)
      | ^^^^
    "#);
}

#[test]
fn unnamed_def_allowed_as_last() {
    let input = indoc! {r#"
    Expr = (identifier)
    (program (Expr) @value)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "Expr"
        Equals "="
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
      Def
        Tree
          ParenOpen "("
          LowerIdent "program"
          Capture
            Tree
              ParenOpen "("
              UpperIdent "Expr"
              ParenClose ")"
            At "@"
            LowerIdent "value"
          ParenClose ")"
    "#);
}

#[test]
fn unnamed_def_not_allowed_in_middle() {
    let input = indoc! {r#"
    (first)
    Expr = (identifier)
    (last)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "first"
          ParenClose ")"
      Def
        UpperIdent "Expr"
        Equals "="
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
      Def
        Tree
          ParenOpen "("
          LowerIdent "last"
          ParenClose ")"
    ---
    error: unnamed definition must be last in file; add a name: `Name = (first)`
      |
    1 | (first)
      | ^^^^^^^
    "#);
}

#[test]
fn multiple_unnamed_defs_errors_for_all_but_last() {
    let input = indoc! {r#"
    (first)
    (second)
    (third)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "first"
          ParenClose ")"
      Def
        Tree
          ParenOpen "("
          LowerIdent "second"
          ParenClose ")"
      Def
        Tree
          ParenOpen "("
          LowerIdent "third"
          ParenClose ")"
    ---
    error: unnamed definition must be last in file; add a name: `Name = (first)`
      |
    1 | (first)
      | ^^^^^^^
    error: unnamed definition must be last in file; add a name: `Name = (second)`
      |
    2 | (second)
      | ^^^^^^^^
    "#);
}
