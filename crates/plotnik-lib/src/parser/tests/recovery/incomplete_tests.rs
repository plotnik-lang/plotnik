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
    error: expected name after `@`
      |
    1 | (identifier) @
      |               ^
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
    error: expected an expression; after `field:`
      |
    1 | (call name:)
      |            ^
    ");
}

#[test]
fn named_def_eof_after_equals() {
    let input = "Expr = ";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected an expression; after `=` in definition
      |
    1 | Expr = 
      |        ^
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
    error: expected type name after `::`; e.g., `::MyType` or `::string`
      |
    1 | (identifier) @name ::
      |                      ^
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
    error: expected field name; e.g., `!value`
      |
    1 | (call !)
      |        ^
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
    error: expected subtype after `/`; e.g., `expression/binary_expression`
      |
    1 | (expression/)
      |             ^
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
    error: expected an expression; after `Label:`
      |
    1 | [Label:]
      |        ^
    ");
}

#[test]
fn type_annotation_missing_name_at_eof() {
    let input = "(a) @x ::";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected type name after `::`; e.g., `::MyType` or `::string`
      |
    1 | (a) @x ::
      |          ^
    ");
}

#[test]
fn type_annotation_missing_name_with_bracket() {
    let input = "[(a) @x :: ]";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected type name after `::`; e.g., `::MyType` or `::string`
      |
    1 | [(a) @x :: ]
      |            ^
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
    error: expected type name after `::`; e.g., `::MyType` or `::string`
      |
    1 | (identifier) @name :: (
      |                       ^
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
    error: expected an expression; after `field:`
      |
    1 | (call name: %%%)
      |             ^^^
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
    error: expected name after `@`
      |
    1 | (identifier) @123
      |               ^^^
    ");
}

#[test]
fn bare_capture_at_eof_triggers_sync() {
    let input = "@";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `@` must follow an expression to capture
      |
    1 | @
      | ^
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
    error: `@` must follow an expression to capture
      |
    1 | @name
      | ^

    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | @name
      |  ^^^^
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
    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | [@x (a)]
      |   ^
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
    error: `@` must follow an expression to capture
      |
    1 | (a) @ok @ @name
      |         ^

    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | (a) @ok @ @name
      |            ^^^^
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
    error: expected an expression; after `field =`
      |
    1 | (call name = )
      |              ^
    ");
}

#[test]
fn lowercase_branch_label_missing_expression() {
    let input = "[label:]";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: expected an expression; after `label:`
      |
    1 | [label:]
      |        ^
    ");
}
