use crate::Query;
use indoc::indoc;

#[test]
fn missing_capture_name() {
    let input = indoc! {r#"
    (identifier) @
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected capture name after '@'
      |
    1 | (identifier) @
      |               ^ expected capture name after '@'
    ");
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
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | (identifier) @name ::
      |                      ^ expected type name after '::' (e.g., ::MyType or ::string)
    ");
}

#[test]
fn missing_negated_field_name() {
    let input = indoc! {r#"
    (call !)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected field name after '!' (e.g., !value)
      |
    1 | (call !)
      |        ^ expected field name after '!' (e.g., !value)
    ");
}

#[test]
fn missing_subtype() {
    let input = indoc! {r#"
    (expression/)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected subtype after '/' (e.g., expression/binary_expression)
      |
    1 | (expression/)
      |             ^ expected subtype after '/' (e.g., expression/binary_expression)
    ");
}

#[test]
fn tagged_branch_missing_expression() {
    let input = indoc! {r#"
    [Label:]
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected expression after branch label
      |
    1 | [Label:]
      |        ^ expected expression after branch label
    ");
}

#[test]
fn type_annotation_missing_name_at_eof() {
    let input = "(a) @x ::";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | (a) @x ::
      |          ^ expected type name after '::' (e.g., ::MyType or ::string)
    ");
}

#[test]
fn type_annotation_missing_name_with_bracket() {
    let input = "[(a) @x :: ]";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | [(a) @x :: ]
      |            ^ expected type name after '::' (e.g., ::MyType or ::string)
    ");
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
fn field_value_is_garbage() {
    let input = indoc! {r#"
    (call name: %%%)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected expression after field name
      |
    1 | (call name: %%%)
      |             ^^^ expected expression after field name
    ");
}

#[test]
fn capture_with_invalid_char() {
    let input = indoc! {r#"
    (identifier) @123
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected capture name after '@'
      |
    1 | (identifier) @123
      |               ^^^ expected capture name after '@'
    ");
}

#[test]
fn bare_capture_at_eof_triggers_sync() {
    let input = "@";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: capture '@' must follow an expression to capture
      |
    1 | @
      | ^ capture '@' must follow an expression to capture
    ");
}

#[test]
fn bare_capture_at_root() {
    let input = indoc! {r#"
    @name
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: capture '@' must follow an expression to capture
      |
    1 | @name
      | ^ capture '@' must follow an expression to capture
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | @name
      |  ^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    ");
}

#[test]
fn capture_at_start_of_alternation() {
    let input = indoc! {r#"
    [@x (a)]
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | [@x (a)]
      |  ^ unexpected token; expected a child expression or closing delimiter
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | [@x (a)]
      |   ^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    ");
}

#[test]
fn mixed_valid_invalid_captures() {
    let input = indoc! {r#"
    (a) @ok @ @name
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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
    ");
}

#[test]
fn field_equals_typo_missing_value() {
    let input = indoc! {r#"
    (call name = )
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: '=' is not valid for field constraints
      |
    1 | (call name = )
      |            ^ '=' is not valid for field constraints
      |
    help: use ':'
      |
    1 - (call name = )
    1 + (call name : )
      |
    error: expected expression after field name
      |
    1 | (call name = )
      |              ^ expected expression after field name
    ");
}

#[test]
fn lowercase_branch_label_missing_expression() {
    let input = "[label:]";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: tagged alternation labels must be Capitalized (they map to enum variants)
      |
    1 | [label:]
      |  ^^^^^ tagged alternation labels must be Capitalized (they map to enum variants)
      |
    help: capitalize as `Label`
      |
    1 - [label:]
    1 + [Label:]
      |
    error: expected expression after branch label
      |
    1 | [label:]
      |        ^ expected expression after branch label
    ");
}
