use crate::Query;
use indoc::indoc;

#[test]
fn missing_capture_name() {
    let input = indoc! {r#"
    (identifier) @
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected capture name: expected capture name
      |
    1 | (identifier) @
      |               ^ expected capture name: expected capture name
    ");
}

#[test]
fn missing_field_value() {
    let input = indoc! {r#"
    (call name:)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected expression: expected expression after field name
      |
    1 | (call name:)
      |            ^ expected expression: expected expression after field name
    ");
}

#[test]
fn named_def_eof_after_equals() {
    let input = "Expr = ";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected expression: expected expression after '=' in named definition
      |
    1 | Expr = 
      |        ^ expected expression: expected expression after '=' in named definition
    ");
}

#[test]
fn missing_type_name() {
    let input = indoc! {r#"
    (identifier) @name ::
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected type name: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | (identifier) @name ::
      |                      ^ expected type name: expected type name after '::' (e.g., ::MyType or ::string)
    ");
}

#[test]
fn missing_negated_field_name() {
    let input = indoc! {r#"
    (call !)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected field name: expected field name after '!' (e.g., !value)
      |
    1 | (call !)
      |        ^ expected field name: expected field name after '!' (e.g., !value)
    ");
}

#[test]
fn missing_subtype() {
    let input = indoc! {r#"
    (expression/)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected subtype: expected subtype after '/' (e.g., expression/binary_expression)
      |
    1 | (expression/)
      |             ^ expected subtype: expected subtype after '/' (e.g., expression/binary_expression)
    ");
}

#[test]
fn tagged_branch_missing_expression() {
    let input = indoc! {r#"
    [Label:]
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected expression: expected expression after branch label
      |
    1 | [Label:]
      |        ^ expected expression: expected expression after branch label
    ");
}

#[test]
fn type_annotation_missing_name_at_eof() {
    let input = "(a) @x ::";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected type name: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | (a) @x ::
      |          ^ expected type name: expected type name after '::' (e.g., ::MyType or ::string)
    ");
}

#[test]
fn type_annotation_missing_name_with_bracket() {
    let input = "[(a) @x :: ]";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected type name: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | [(a) @x :: ]
      |            ^ expected type name: expected type name after '::' (e.g., ::MyType or ::string)
    ");
}

#[test]
fn type_annotation_invalid_token_after() {
    let input = indoc! {r#"
    (identifier) @name :: (
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected type name: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | (identifier) @name :: (
      |                       ^ expected type name: expected type name after '::' (e.g., ::MyType or ::string)
    error: unclosed tree: unclosed tree; expected ')'
      |
    1 | (identifier) @name :: (
      |                       -^ unclosed tree: unclosed tree; expected ')'
      |                       |
      |                       tree started here
    error: unnamed definition must be last: add a name: `Name = (identifier) @name ::`
      |
    1 | (identifier) @name :: (
      | ^^^^^^^^^^^^^^^^^^^^^ unnamed definition must be last: add a name: `Name = (identifier) @name ::`
    ");
}

#[test]
fn field_value_is_garbage() {
    let input = indoc! {r#"
    (call name: %%%)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected expression: expected expression after field name
      |
    1 | (call name: %%%)
      |             ^^^ expected expression: expected expression after field name
    ");
}

#[test]
fn capture_with_invalid_char() {
    let input = indoc! {r#"
    (identifier) @123
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected capture name: expected capture name
      |
    1 | (identifier) @123
      |               ^^^ expected capture name: expected capture name
    ");
}

#[test]
fn bare_capture_at_eof_triggers_sync() {
    let input = "@";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: capture without target: capture without target
      |
    1 | @
      | ^ capture without target: capture without target
    ");
}

#[test]
fn bare_capture_at_root() {
    let input = indoc! {r#"
    @name
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: capture without target: capture without target
      |
    1 | @name
      | ^ capture without target: capture without target
    error: bare identifier not allowed: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | @name
      |  ^^^^ bare identifier not allowed: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    ");
}

#[test]
fn capture_at_start_of_alternation() {
    let input = indoc! {r#"
    [@x (a)]
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unexpected token: unexpected token; expected a child expression or closing delimiter
      |
    1 | [@x (a)]
      |  ^ unexpected token: unexpected token; expected a child expression or closing delimiter
    error: bare identifier not allowed: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | [@x (a)]
      |   ^ bare identifier not allowed: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    ");
}

#[test]
fn mixed_valid_invalid_captures() {
    let input = indoc! {r#"
    (a) @ok @ @name
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: capture without target: capture without target
      |
    1 | (a) @ok @ @name
      |         ^ capture without target: capture without target
    error: bare identifier not allowed: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | (a) @ok @ @name
      |            ^^^^ bare identifier not allowed: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    error: unnamed definition must be last: add a name: `Name = (a) @ok`
      |
    1 | (a) @ok @ @name
      | ^^^^^^^ unnamed definition must be last: add a name: `Name = (a) @ok`
    ");
}

#[test]
fn field_equals_typo_missing_value() {
    let input = indoc! {r#"
    (call name = )
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: invalid field syntax: '=' is not valid for field constraints
      |
    1 | (call name = )
      |            ^ invalid field syntax: '=' is not valid for field constraints
      |
    help: use ':'
      |
    1 - (call name = )
    1 + (call name : )
      |
    error: expected expression: expected expression after field name
      |
    1 | (call name = )
      |              ^ expected expression: expected expression after field name
    ");
}

#[test]
fn lowercase_branch_label_missing_expression() {
    let input = "[label:]";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: lowercase branch label: tagged alternation labels must be Capitalized (they map to enum variants)
      |
    1 | [label:]
      |  ^^^^^ lowercase branch label: tagged alternation labels must be Capitalized (they map to enum variants)
      |
    help: capitalize as `Label`
      |
    1 - [label:]
    1 + [Label:]
      |
    error: expected expression: expected expression after branch label
      |
    1 | [label:]
      |        ^ expected expression: expected expression after branch label
    ");
}
