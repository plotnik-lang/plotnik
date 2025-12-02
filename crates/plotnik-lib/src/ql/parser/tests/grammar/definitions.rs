use crate::Query;
use indoc::indoc;

#[test]
fn simple_named_def() {
    let input = indoc! {r#"
    Expr = (identifier)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Id "Expr"
        Equals "="
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
    "#);
}

#[test]
fn named_def_with_alternation() {
    let input = indoc! {r#"
    Value = [(identifier) (number) (string)]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Id "Value"
        Equals "="
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
          Branch
            Tree
              ParenOpen "("
              Id "string"
              ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn named_def_with_sequence() {
    let input = indoc! {r#"
    Pair = {(identifier) (expression)}
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Id "Pair"
        Equals "="
        Seq
          BraceOpen "{"
          Tree
            ParenOpen "("
            Id "identifier"
            ParenClose ")"
          Tree
            ParenOpen "("
            Id "expression"
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Id "BinaryOp"
        Equals "="
        Tree
          ParenOpen "("
          Id "binary_expression"
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
              Id "operator"
              Colon ":"
              Wildcard
                Underscore "_"
            At "@"
            Id "op"
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
    "#);
}

#[test]
fn multiple_named_defs() {
    let input = indoc! {r#"
    Expr = (expression)
    Stmt = (statement)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Id "Expr"
        Equals "="
        Tree
          ParenOpen "("
          Id "expression"
          ParenClose ")"
      Def
        Id "Stmt"
        Equals "="
        Tree
          ParenOpen "("
          Id "statement"
          ParenClose ")"
    "#);
}

#[test]
fn named_def_then_pattern() {
    let input = indoc! {r#"
    Expr = [(identifier) (number)]
    (program (Expr) @value)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Id "Expr"
        Equals "="
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
      Def
        Tree
          ParenOpen "("
          Id "program"
          Capture
            Ref
              ParenOpen "("
              Id "Expr"
              ParenClose ")"
            At "@"
            Id "value"
          ParenClose ")"
    "#);
}

#[test]
fn named_def_referencing_another() {
    let input = indoc! {r#"
    Literal = [(number) (string)]
    Expr = [(identifier) (Literal)]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Id "Literal"
        Equals "="
        Alt
          BracketOpen "["
          Branch
            Tree
              ParenOpen "("
              Id "number"
              ParenClose ")"
          Branch
            Tree
              ParenOpen "("
              Id "string"
              ParenClose ")"
          BracketClose "]"
      Def
        Id "Expr"
        Equals "="
        Alt
          BracketOpen "["
          Branch
            Tree
              ParenOpen "("
              Id "identifier"
              ParenClose ")"
          Branch
            Ref
              ParenOpen "("
              Id "Literal"
              ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn named_def_with_quantifier() {
    let input = indoc! {r#"
    Statements = (statement)+
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Id "Statements"
        Equals "="
        Quantifier
          Tree
            ParenOpen "("
            Id "statement"
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Id "NestedCall"
        Equals "="
        Tree
          ParenOpen "("
          Id "call_expression"
          Field
            Id "function"
            Colon ":"
            Alt
              BracketOpen "["
              Branch
                Capture
                  Tree
                    ParenOpen "("
                    Id "identifier"
                    ParenClose ")"
                  At "@"
                  Id "name"
              Branch
                Capture
                  Ref
                    ParenOpen "("
                    Id "NestedCall"
                    ParenClose ")"
                  At "@"
                  Id "inner"
              BracketClose "]"
          Field
            Id "arguments"
            Colon ":"
            Tree
              ParenOpen "("
              Id "arguments"
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Id "Func"
        Equals "="
        Tree
          ParenOpen "("
          Id "function_declaration"
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
          Capture
            Field
              Id "body"
              Colon ":"
              Tree
                ParenOpen "("
                Underscore "_"
                ParenClose ")"
            At "@"
            Id "body"
          ParenClose ")"
    "#);
}

#[test]
fn upper_ident_not_followed_by_equals_is_pattern() {
    let input = indoc! {r#"
    (Expr)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Ref
          ParenOpen "("
          Id "Expr"
          ParenClose ")"
    ---
    error: undefined reference: `Expr`
      |
    1 | (Expr)
      |  ^^^^ undefined reference: `Expr`
    "#);
}

#[test]
fn bare_upper_ident_not_followed_by_equals_is_error() {
    let input = indoc! {r#"
    Expr
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Error
          Id "Expr"
    ---
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | Expr
      | ^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    "#);
}

#[test]
fn named_def_missing_equals() {
    let input = indoc! {r#"
    Expr (identifier)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Error
          Id "Expr"
      Def
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
    ---
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | Expr (identifier)
      | ^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    error: unnamed definition must be last in file; add a name: `Name = Expr`
      |
    1 | Expr (identifier)
      | ^^^^ unnamed definition must be last in file; add a name: `Name = Expr`
    "#);
}

#[test]
fn unnamed_def_allowed_as_last() {
    let input = indoc! {r#"
    Expr = (identifier)
    (program (Expr) @value)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Id "Expr"
        Equals "="
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
      Def
        Tree
          ParenOpen "("
          Id "program"
          Capture
            Ref
              ParenOpen "("
              Id "Expr"
              ParenClose ")"
            At "@"
            Id "value"
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "first"
          ParenClose ")"
      Def
        Id "Expr"
        Equals "="
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
      Def
        Tree
          ParenOpen "("
          Id "last"
          ParenClose ")"
    ---
    error: unnamed definition must be last in file; add a name: `Name = (first)`
      |
    1 | (first)
      | ^^^^^^^ unnamed definition must be last in file; add a name: `Name = (first)`
    "#);
}

#[test]
fn multiple_unnamed_defs_errors_for_all_but_last() {
    let input = indoc! {r#"
    (first)
    (second)
    (third)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "first"
          ParenClose ")"
      Def
        Tree
          ParenOpen "("
          Id "second"
          ParenClose ")"
      Def
        Tree
          ParenOpen "("
          Id "third"
          ParenClose ")"
    ---
    error: unnamed definition must be last in file; add a name: `Name = (first)`
      |
    1 | (first)
      | ^^^^^^^ unnamed definition must be last in file; add a name: `Name = (first)`
    error: unnamed definition must be last in file; add a name: `Name = (second)`
      |
    2 | (second)
      | ^^^^^^^^ unnamed definition must be last in file; add a name: `Name = (second)`
    "#);
}
