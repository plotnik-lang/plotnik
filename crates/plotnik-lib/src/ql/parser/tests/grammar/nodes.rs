use crate::Query;
use indoc::indoc;

// ============================================================================
// Named Nodes
// ============================================================================

#[test]
fn empty_input() {
    let query = Query::new("");
    insta::assert_snapshot!(query.snapshot_cst(), @"Root");
}

#[test]
fn simple_named_node() {
    let input = indoc! {r#"
    (identifier)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
    "#);
}

#[test]
fn nested_node() {
    let input = indoc! {r#"
    (function_definition name: (identifier))
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "function_definition"
          Field
            Id "name"
            Colon ":"
            Tree
              ParenOpen "("
              Id "identifier"
              ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn deeply_nested() {
    let input = indoc! {r#"
    (a
        (b
        (c
            (d))))
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "a"
          Tree
            ParenOpen "("
            Id "b"
            Tree
              ParenOpen "("
              Id "c"
              Tree
                ParenOpen "("
                Id "d"
                ParenClose ")"
              ParenClose ")"
            ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn sibling_children() {
    let input = indoc! {r#"
    (block
        (statement)
        (statement)
        (statement))
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "block"
          Tree
            ParenOpen "("
            Id "statement"
            ParenClose ")"
          Tree
            ParenOpen "("
            Id "statement"
            ParenClose ")"
          Tree
            ParenOpen "("
            Id "statement"
            ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn multiple_patterns() {
    let input = indoc! {r#"
    (identifier)
    (string)
    (number)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
      Def
        Tree
          ParenOpen "("
          Id "string"
          ParenClose ")"
      Def
        Tree
          ParenOpen "("
          Id "number"
          ParenClose ")"
    ---
    error: unnamed definition must be last in file; add a name: `Name = (identifier)`
      |
    1 | (identifier)
      | ^^^^^^^^^^^^ unnamed definition must be last in file; add a name: `Name = (identifier)`
    error: unnamed definition must be last in file; add a name: `Name = (string)`
      |
    2 | (string)
      | ^^^^^^^^ unnamed definition must be last in file; add a name: `Name = (string)`
    "#);
}

// ============================================================================
// Wildcards
// ============================================================================

#[test]
fn wildcard() {
    let input = indoc! {r#"
    (_)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Underscore "_"
          ParenClose ")"
    "#);
}

// ============================================================================
// Anonymous Nodes (Literals)
// ============================================================================

#[test]
fn anonymous_node() {
    let input = indoc! {r#"
    "if"
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Lit
          StringLit "\"if\""
    "#);
}

#[test]
fn anonymous_node_operator() {
    let input = indoc! {r#"
    "+="
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Lit
          StringLit "\"+=\""
    "#);
}

// ============================================================================
// Supertypes
// ============================================================================

#[test]
fn supertype_basic() {
    let input = indoc! {r#"
    (expression/binary_expression)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "expression"
          Slash "/"
          Id "binary_expression"
          ParenClose ")"
    "#);
}

#[test]
fn supertype_with_string_subtype() {
    let input = indoc! {r#"
    (expression/"()")
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "expression"
          Slash "/"
          StringLit "\"()\""
          ParenClose ")"
    "#);
}

#[test]
fn supertype_with_capture() {
    let input = indoc! {r#"
    (expression/binary_expression) @expr
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            Id "expression"
            Slash "/"
            Id "binary_expression"
            ParenClose ")"
          At "@"
          Id "expr"
    "#);
}

#[test]
fn supertype_with_children() {
    let input = indoc! {r#"
    (expression/binary_expression
        left: (_) @left
        right: (_) @right)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "expression"
          Slash "/"
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
fn supertype_nested() {
    let input = indoc! {r#"
    (statement/expression_statement
        (expression/call_expression))
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "statement"
          Slash "/"
          Id "expression_statement"
          Tree
            ParenOpen "("
            Id "expression"
            Slash "/"
            Id "call_expression"
            ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn supertype_in_alternation() {
    let input = indoc! {r#"
    [(expression/identifier) (expression/number)]
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Branch
            Tree
              ParenOpen "("
              Id "expression"
              Slash "/"
              Id "identifier"
              ParenClose ")"
          Branch
            Tree
              ParenOpen "("
              Id "expression"
              Slash "/"
              Id "number"
              ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn no_supertype_plain_node() {
    let input = indoc! {r#"
    (identifier)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
    "#);
}
