use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

#[test]
fn missing_capture_name() {
    let input = indoc! {r#"
    (identifier) @
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
    ---
    error: expected capture name after '@' (e.g., @name, @my_var)
      |
    1 | (identifier) @
      |               ^
    "#);
}

#[test]
fn missing_field_value() {
    let input = indoc! {r#"
    (call name:)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "name"
          Colon ":"
          Error
            ParenClose ")"
    ---
    error: unexpected token; expected a pattern
      |
    1 | (call name:)
      |            ^
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | (call name:)
      |             ^
    "#);
}

#[test]
fn named_def_eof_after_equals() {
    let input = "Expr = ";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "Expr"
        Equals "="
    ---
    error: expected pattern after '=' in named definition
      |
    1 | Expr = 
      |        ^
    "#);
}

#[test]
fn missing_type_name() {
    let input = indoc! {r#"
    (identifier) @name ::
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "name"
        Type
          DoubleColon "::"
    ---
    error: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | (identifier) @name ::
      |                      ^
    "#);
}

#[test]
fn missing_negated_field_name() {
    let input = indoc! {r#"
    (call !)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "call"
        NegatedField
          Negation "!"
        ParenClose ")"
    ---
    error: expected field name after '!' (e.g., !value)
      |
    1 | (call !)
      |        ^
    "#);
}

#[test]
fn missing_subtype() {
    let input = indoc! {r#"
    (expression/)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
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
fn tagged_branch_missing_pattern() {
    let input = indoc! {r#"
    [Label:]
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Branch
          UpperIdent "Label"
          Colon ":"
        BracketClose "]"
    ---
    error: expected pattern after label in alternation branch
      |
    1 | [Label:]
      |        ^
    "#);
}

#[test]
fn mixed_valid_invalid_captures() {
    let input = indoc! {r#"
    (a) @ok @ @name
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "a"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "ok"
      Capture
        At "@"
      Capture
        At "@"
        LowerIdent "name"
    ---
    error: expected capture name after '@' (e.g., @name, @my_var)
      |
    1 | (a) @ok @ @name
      |           ^
    "#);
}

#[test]
fn type_annotation_invalid_token_after() {
    let input = indoc! {r#"
    (identifier) @name :: (
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "name"
        Type
          DoubleColon "::"
      Node
        ParenOpen "("
    ---
    error: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | (identifier) @name :: (
      |                       ^
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | (identifier) @name :: (
      |                        ^
    "#);
}

#[test]
fn error_with_unexpected_content() {
    let input = indoc! {r#"
    (ERROR (something))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        KwError "ERROR"
        Node
          ParenOpen "("
          LowerIdent "something"
          ParenClose ")"
        ParenClose ")"
    ---
    error: (ERROR) takes no arguments
      |
    1 | (ERROR (something))
      |        ^
    "#);
}

#[test]
fn bare_error_keyword() {
    let input = indoc! {r#"
    ERROR
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Error
        KwError "ERROR"
    ---
    error: ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)
      |
    1 | ERROR
      | ^^^^^
    "#);
}

#[test]
fn bare_missing_keyword() {
    let input = indoc! {r#"
    MISSING
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Error
        KwMissing "MISSING"
    ---
    error: ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)
      |
    1 | MISSING
      | ^^^^^^^
    "#);
}

#[test]
fn deep_nesting_within_limit() {
    let depth = 100;
    let mut input = String::new();
    for _ in 0..depth {
        input.push_str("(a ");
    }
    for _ in 0..depth {
        input.push(')');
    }

    let result = crate::ql::parser::parse(&input);
    assert!(
        result.is_valid(),
        "expected no errors for depth {}, got: {:?}",
        depth,
        result.errors()
    );
}