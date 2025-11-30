use super::helpers_test::*;
use indoc::indoc;

#[test]
fn supertype_basic() {
    let input = indoc! {r#"
    (expression/binary_expression)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
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
      NamedNode
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
      NamedNode
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
      NamedNode
        ParenOpen "("
        LowerIdent "expression"
        Slash "/"
        LowerIdent "binary_expression"
        Field
          LowerIdent "left"
          Colon ":"
          NamedNode
            ParenOpen "("
            Underscore "_"
            ParenClose ")"
        Capture
          At "@"
          LowerIdent "left"
        Field
          LowerIdent "right"
          Colon ":"
          NamedNode
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
      NamedNode
        ParenOpen "("
        LowerIdent "statement"
        Slash "/"
        LowerIdent "expression_statement"
        NamedNode
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
      Alternation
        BracketOpen "["
        NamedNode
          ParenOpen "("
          LowerIdent "expression"
          Slash "/"
          LowerIdent "identifier"
          ParenClose ")"
        NamedNode
          ParenOpen "("
          LowerIdent "expression"
          Slash "/"
          LowerIdent "number"
          ParenClose ")"
        BracketClose "]"
    "#);
}

#[test]
fn supertype_missing_subtype() {
    let input = indoc! {r#"
    (expression/)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "expression"
        Slash "/"
        ParenClose ")"
    ---
    error: expected subtype after '/' (e.g., expression/binary_expression)
      |
    1 | (expression/)
      |             ^
    "#);
}

#[test]
fn no_supertype_plain_node() {
    let input = indoc! {r#"
    (identifier)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
    "#);
}
