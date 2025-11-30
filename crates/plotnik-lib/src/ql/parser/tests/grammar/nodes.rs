use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

// ============================================================================
// Named Nodes
// ============================================================================

#[test]
fn empty_input() {
    insta::assert_snapshot!(snapshot(""), @"Root");
}

#[test]
fn simple_named_node() {
    let input = indoc! {r#"
    (identifier)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
    "#);
}

#[test]
fn nested_node() {
    let input = indoc! {r#"
    (function_definition name: (identifier))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "function_definition"
        Field
          LowerIdent "name"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "identifier"
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "a"
        Node
          ParenOpen "("
          LowerIdent "b"
          Node
            ParenOpen "("
            LowerIdent "c"
            Node
              ParenOpen "("
              LowerIdent "d"
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "block"
        Node
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "statement"
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Node
        ParenOpen "("
        LowerIdent "string"
        ParenClose ")"
      Node
        ParenOpen "("
        LowerIdent "number"
        ParenClose ")"
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Lit
        StringLit "\"if\""
    "#);
}

#[test]
fn anonymous_node_operator() {
    let input = indoc! {r#"
    "+="
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "expression"
        Slash "/"
        LowerIdent "binary_expression"
        ParenClose ")"
    "#);
}

#[test]
fn supertype_with_string_subtype() {
    let input = indoc! {r#"
    (expression/"()")
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "expression"
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

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "expression"
        Slash "/"
        LowerIdent "binary_expression"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "expr"
    "#);
}

#[test]
fn supertype_with_children() {
    let input = indoc! {r#"
    (expression/binary_expression
        left: (_) @left
        right: (_) @right)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "expression"
        Slash "/"
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
fn supertype_nested() {
    let input = indoc! {r#"
    (statement/expression_statement
        (expression/call_expression))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "statement"
        Slash "/"
        LowerIdent "expression_statement"
        Node
          ParenOpen "("
          LowerIdent "expression"
          Slash "/"
          LowerIdent "call_expression"
          ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn supertype_in_alternation() {
    let input = indoc! {r#"
    [(expression/identifier) (expression/number)]
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Node
          ParenOpen "("
          LowerIdent "expression"
          Slash "/"
          LowerIdent "identifier"
          ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "expression"
          Slash "/"
          LowerIdent "number"
          ParenClose ")"
        BracketClose "]"
    "#);
}

#[test]
fn no_supertype_plain_node() {
    let input = indoc! {r#"
    (identifier)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
    "#);
}