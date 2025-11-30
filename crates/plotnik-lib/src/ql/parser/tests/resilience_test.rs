use super::helpers_test::*;
use indoc::indoc;

#[test]
fn deep_nesting_within_limit() {
    // Test that nesting up to depth limit works without error.
    // Note: In debug builds, the fuel mechanism (256 iterations without progress)
    // may trigger before the recursion limit (512) on very deep nesting.
    // We test a moderate depth that works reliably in both debug and release.
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

#[test]
fn top_level_garbage_recovery() {
    // Use actual invalid tokens (^^^) since lowercase identifiers are valid patterns
    let input = indoc! {r#"
    Expr = (a) ^^^ Expr2 = (b)
    "#};

    // Parser should recover and still parse Expr2
    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "Expr"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
      Error
        Garbage "^^^"
      Def
        UpperIdent "Expr2"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
    ---
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    1 | Expr = (a) ^^^ Expr2 = (b)
      |            ^^^
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
fn field_with_upper_ident_parses() {
    // Parser accepts UpperIdent as field name for resilience; validator will catch casing errors
    let input = indoc! {r#"
    (node FieldTypo: (x))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "node"
        Field
          UpperIdent "FieldTypo"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "x"
            ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn capture_with_upper_ident_parses() {
    // Parser accepts UpperIdent as capture name for resilience; validator will catch casing errors
    let input = indoc! {r#"
    (identifier) @Name
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        UpperIdent "Name"
    "#);
}

#[test]
fn negated_field_with_upper_ident_parses() {
    // Parser should accept UpperIdent as negated field name for resilience
    let input = indoc! {r#"
    (call !Arguments)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "call"
        NegatedField
          Negation "!"
          UpperIdent "Arguments"
        ParenClose ")"
    "#);
}

#[test]
fn multiple_definitions_with_garbage_between() {
    let input = indoc! {r#"
    A = (a)
    ^^^
    B = (b)
    $$$
    C = (c)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "A"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
      Error
        Garbage "^^^"
      Def
        UpperIdent "B"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
      Error
        Garbage "$$$"
      Def
        UpperIdent "C"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "c"
          ParenClose ")"
    ---
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    2 | ^^^
      | ^^^
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    4 | $$$
      | ^^^
    "#);
}

#[test]
fn capture_with_type_and_upper_ident() {
    // Even if capture name is UpperIdent, type annotation should still work
    let input = indoc! {r#"
    (identifier) @Name::MyType
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        UpperIdent "Name"
        Type
          DoubleColon "::"
          UpperIdent "MyType"
    "#);
}
