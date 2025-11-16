use crate::Query;
use indoc::indoc;

#[test]
fn missing_capture_name() {
    let input = indoc! {r#"
    (identifier) @
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: expected capture name after '@'
      |
    1 | (identifier) @
      |               ^ expected capture name after '@'
    "#);
}

#[test]
fn missing_field_value() {
    let input = indoc! {r#"
    (call name:)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected expression after field name
      |
    1 | (call name:)
      |            ^ expected expression after field name
    ");
}

#[test]
fn named_def_eof_after_equals() {
    let input = "Expr = ";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected expression after '=' in named definition
      |
    1 | Expr = 
      |        ^ expected expression after '=' in named definition
    ");
}

#[test]
fn missing_type_name() {
    let input = indoc! {r#"
    (identifier) @name ::
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | (identifier) @name ::
      |                      ^ expected type name after '::' (e.g., ::MyType or ::string)
    "#);
}

#[test]
fn missing_negated_field_name() {
    let input = indoc! {r#"
    (call !)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: expected field name after '!' (e.g., !value)
      |
    1 | (call !)
      |        ^ expected field name after '!' (e.g., !value)
    "#);
}

#[test]
fn missing_subtype() {
    let input = indoc! {r#"
    (expression/)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: expected subtype after '/' (e.g., expression/binary_expression)
      |
    1 | (expression/)
      |             ^ expected subtype after '/' (e.g., expression/binary_expression)
    "#);
}

#[test]
fn tagged_branch_missing_expression() {
    let input = indoc! {r#"
    [Label:]
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: expected expression after branch label
      |
    1 | [Label:]
      |        ^ expected expression after branch label
    "#);
}

#[test]
fn mixed_valid_invalid_captures() {
    let input = indoc! {r#"
    (a) @ok @ @name
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: capture '@' must follow an expression to capture
      |
    1 | (a) @ok @ @name
      |         ^ capture '@' must follow an expression to capture
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | (a) @ok @ @name
      |            ^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    error: unnamed definition must be last in file; add a name: `Name = (a) @ok`
      |
    1 | (a) @ok @ @name
      | ^^^^^^^ unnamed definition must be last in file; add a name: `Name = (a) @ok`
    "#);
}

#[test]
fn type_annotation_invalid_token_after() {
    let input = indoc! {r#"
    (identifier) @name :: (
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | (identifier) @name :: (
      |                       ^ expected type name after '::' (e.g., ::MyType or ::string)
    error: unclosed tree; expected ')'
      |
    1 | (identifier) @name :: (
      |                       -^ unclosed tree; expected ')'
      |                       |
      |                       tree started here
    error: unnamed definition must be last in file; add a name: `Name = (identifier) @name ::`
      |
    1 | (identifier) @name :: (
      | ^^^^^^^^^^^^^^^^^^^^^ unnamed definition must be last in file; add a name: `Name = (identifier) @name ::`
    ");
}

#[test]
fn error_with_unexpected_content() {
    let input = indoc! {r#"
    (ERROR (something))
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: (ERROR) takes no arguments
      |
    1 | (ERROR (something))
      |        ^ (ERROR) takes no arguments
    "#);
}

#[test]
fn bare_error_keyword() {
    let input = indoc! {r#"
    ERROR
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)
      |
    1 | ERROR
      | ^^^^^ ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)
    "#);
}

#[test]
fn bare_missing_keyword() {
    let input = indoc! {r#"
    MISSING
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)
      |
    1 | MISSING
      | ^^^^^^^ ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)
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

    let result = crate::ast::parser::parse(&input).unwrap();
    assert!(
        result.is_valid(),
        "expected no errors for depth {}, got: {:?}",
        depth,
        result.errors()
    );
}
