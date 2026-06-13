use crate::Query;
use indoc::indoc;

#[test]
fn missing_capture_name() {
    let input = indoc! {r#"
    (identifier) @
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: expected a capture name after `@`
      |
    1 | (identifier) @
      |              ^
      |
    help: captures attach to the pattern before them: `(node) @name`
    ");
}

#[test]
fn missing_field_value() {
    let input = indoc! {r#"
    (call name:)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: expected an expression
      |
    1 | (call name:)
      |            ^
    ");
}

#[test]
fn named_def_eof_after_equals() {
    let input = "Expr = ";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: expected an expression
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: expected a type name after `::`
      |
    1 | (identifier) @name ::
      |                      ^
      |
    help: e.g., `::MyType` or `::string`
    ");
}

#[test]
fn missing_negated_field_name() {
    let input = indoc! {r#"
    (call -)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: expected a field name
      |
    1 | (call -)
      |        ^
      |
    help: e.g., `-value`
    ");
}

#[test]
fn missing_subtype() {
    let input = indoc! {r#"
    (expression/)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: expected a subtype after `/`
      |
    1 | (expression/)
      |             ^
      |
    help: e.g., `expression#binary_expression`
    ");
}

#[test]
fn tagged_branch_missing_expression() {
    let input = indoc! {r#"
    [Label:]
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: expected an expression
      |
    1 | [Label:]
      |        ^
    ");
}

#[test]
fn type_annotation_missing_name_at_eof() {
    let input = "(a) @x ::";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: expected a type name after `::`
      |
    1 | (a) @x ::
      |          ^
      |
    help: e.g., `::MyType` or `::string`
    ");
}

#[test]
fn type_annotation_missing_name_with_bracket() {
    let input = "[(a) @x :: ]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: expected a type name after `::`
      |
    1 | [(a) @x :: ]
      |            ^
      |
    help: e.g., `::MyType` or `::string`
    ");
}

#[test]
fn type_annotation_invalid_token_after() {
    let input = indoc! {r#"
    (identifier) @name :: (
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: expected a type name after `::`
      |
    1 | (identifier) @name :: (
      |                       ^
      |
    help: e.g., `::MyType` or `::string`
    ");
}

#[test]
fn field_value_is_garbage() {
    let input = indoc! {r#"
    (call name: %%%)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: expected an expression
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: expected a capture name after `@`
      |
    1 | (identifier) @123
      |              ^
      |
    help: captures attach to the pattern before them: `(node) @name`
    ");
}

#[test]
fn bare_capture_at_eof_triggers_sync() {
    let input = "@";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: expected a capture name after `@`
      |
    1 | @
      | ^
      |
    help: captures attach to the pattern before them: `(node) @name`
    ");
}

#[test]
fn bare_capture_at_root() {
    let input = indoc! {r#"
    @name
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: unexpected token
      |
    1 | @name
      | ^^^^^
      |
    help: try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
    "#);
}

#[test]
fn capture_at_start_of_alternation() {
    let input = indoc! {r#"
    [@x (a)]
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: unexpected token
      |
    1 | [@x (a)]
      |  ^^
      |
    help: expected a branch, or `]` to close
    ");
}

#[test]
fn mixed_valid_invalid_captures() {
    let input = indoc! {r#"
    (a) @ok @ @name
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: expected a capture name after `@`
      |
    1 | (a) @ok @ @name
      |         ^
      |
    help: captures attach to the pattern before them: `(node) @name`
    ");
}

#[test]
fn field_equals_typo_missing_value() {
    let input = indoc! {r#"
    (call name = )
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: fields use `:`, not `=`
      |
    1 | (call name = )
      |            ^
      |
    help: use `:`
      |
    1 - (call name = )
    1 + (call name : )
      |

    error: expected an expression
      |
    1 | (call name = )
      |              ^
    ");
}

#[test]
fn lowercase_branch_label_missing_expression() {
    let input = "[label:]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"
    error: branch labels must be PascalCase
      |
    1 | [label:]
      |  ^^^^^
      |
    help: use `Label`
      |
    1 - [label:]
    1 + [Label:]
      |
    help: branch labels become enum variants in the output

    error: expected an expression
      |
    1 | [label:]
      |        ^
    ");
}

#[test]
fn unterminated_string_in_node() {
    let input = indoc! {r#"
    (call "abc
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: missing closing quote
      |
    1 | (call "abc
      |       ^^^^
    "#);
}

#[test]
fn unterminated_string_in_predicate() {
    let input = indoc! {r#"
    (name == "foo
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: missing closing quote
      |
    1 | (name == "foo
      |          ^^^^
    "#);
}

#[test]
fn unterminated_string_swallows_closing_paren() {
    let input = indoc! {r#"
    Q = ("abc)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r#"
    error: missing closing quote
      |
    1 | Q = ("abc)
      |      ^^^^^
    "#);
}
