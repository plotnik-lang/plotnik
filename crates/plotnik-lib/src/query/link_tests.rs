use crate::Query;
use indoc::indoc;
use plotnik_langs::Lang;
use std::sync::LazyLock;

static LANG: LazyLock<Lang> = LazyLock::new(|| plotnik_langs::javascript());

#[test]
fn valid_query_with_field() {
    let input = indoc! {r#"
        Q = (function_declaration
            name: (identifier) @name) @fn
    "#};
    Query::expect_valid_linking(input, &LANG);
}

#[test]
fn unknown_node_type_with_suggestion() {
    let input = indoc! {r#"
        Q = (function_declaraton) @fn
    "#};

    let res = Query::expect_invalid_linking(input, &LANG);

    insta::assert_snapshot!(res, @r"
    error: `function_declaraton` is not a valid node type
      |
    1 | Q = (function_declaraton) @fn
      |      ^^^^^^^^^^^^^^^^^^^
      |
    help: did you mean `function_declaration`?
    ");
}

#[test]
fn unknown_node_type_no_suggestion() {
    let input = indoc! {r#"
        Q = (xyzzy_foobar_baz) @fn
    "#};

    let res = Query::expect_invalid_linking(input, &LANG);

    insta::assert_snapshot!(res, @r"
    error: `xyzzy_foobar_baz` is not a valid node type
      |
    1 | Q = (xyzzy_foobar_baz) @fn
      |      ^^^^^^^^^^^^^^^^
    ");
}

#[test]
fn unknown_field_with_suggestion() {
    let input = indoc! {r#"
        Q = (function_declaration
            nme: (identifier) @name) @fn
    "#};

    let res = Query::expect_invalid_linking(input, &LANG);

    insta::assert_snapshot!(res, @r"
    error: `nme` is not a valid field
      |
    2 |     nme: (identifier) @name) @fn
      |     ^^^
      |
    help: did you mean `name`?
    ");
}

#[test]
fn unknown_field_no_suggestion() {
    let input = indoc! {r#"
        Q = (function_declaration
            xyzzy: (identifier) @name) @fn
    "#};

    let res = Query::expect_invalid_linking(input, &LANG);

    insta::assert_snapshot!(res, @r"
    error: `xyzzy` is not a valid field
      |
    2 |     xyzzy: (identifier) @name) @fn
      |     ^^^^^
    ");
}

#[test]
fn field_not_on_node_type() {
    let input = indoc! {r#"
        Q = (function_declaration
            condition: (identifier) @name) @fn
    "#};

    let res = Query::expect_invalid_linking(input, &LANG);

    insta::assert_snapshot!(res, @r"
    error: field `condition` is not valid on this node type
      |
    1 | Q = (function_declaration
      |      -------------------- on `function_declaration`
    2 |     condition: (identifier) @name) @fn
      |     ^^^^^^^^^
      |
    help: valid fields for `function_declaration`: `body`, `name`, `parameters`
    ");
}

#[test]
fn field_not_on_node_type_with_suggestion() {
    let input = indoc! {r#"
        Q = (function_declaration
            parameter: (formal_parameters) @params
        ) @fn
    "#};

    let res = Query::expect_invalid_linking(input, &LANG);

    insta::assert_snapshot!(res, @r"
    error: field `parameter` is not valid on this node type
      |
    1 | Q = (function_declaration
      |      -------------------- on `function_declaration`
    2 |     parameter: (formal_parameters) @params
      |     ^^^^^^^^^
      |
    help: did you mean `parameters`?
    help: valid fields for `function_declaration`: `body`, `name`, `parameters`
    ");
}

#[test]
fn negated_field_unknown() {
    let input = indoc! {r#"
        Q = (function_declaration !nme) @fn
    "#};

    let res = Query::expect_invalid_linking(input, &LANG);

    insta::assert_snapshot!(res, @r"
    error: `nme` is not a valid field
      |
    1 | Q = (function_declaration !nme) @fn
      |                            ^^^
      |
    help: did you mean `name`?
    ");
}

#[test]
fn negated_field_not_on_node_type() {
    let input = indoc! {r#"
        Q = (function_declaration !condition) @fn
    "#};

    let res = Query::expect_invalid_linking(input, &LANG);

    insta::assert_snapshot!(res, @r"
    error: field `condition` is not valid on this node type
      |
    1 | Q = (function_declaration !condition) @fn
      |      --------------------  ^^^^^^^^^
      |      |
      |      on `function_declaration`
      |
    help: valid fields for `function_declaration`: `body`, `name`, `parameters`
    ");
}

#[test]
fn negated_field_not_on_node_type_with_suggestion() {
    let input = indoc! {r#"
        Q = (function_declaration !parameter) @fn
    "#};

    let res = Query::expect_invalid_linking(input, &LANG);

    insta::assert_snapshot!(res, @r"
    error: field `parameter` is not valid on this node type
      |
    1 | Q = (function_declaration !parameter) @fn
      |      --------------------  ^^^^^^^^^
      |      |
      |      on `function_declaration`
      |
    help: did you mean `parameters`?
    help: valid fields for `function_declaration`: `body`, `name`, `parameters`
    ");
}

#[test]
fn negated_field_valid() {
    let input = indoc! {r#"
        Q = (function_declaration !name) @fn
    "#};

    Query::expect_valid_linking(input, &LANG);
}

#[test]
fn anonymous_node_unknown() {
    let input = indoc! {r#"
        Q = (function_declaration "xyzzy_fake_token") @fn
    "#};

    let res = Query::expect_invalid_linking(input, &LANG);

    insta::assert_snapshot!(res, @r#"
    error: `xyzzy_fake_token` is not a valid node type
      |
    1 | Q = (function_declaration "xyzzy_fake_token") @fn
      |                            ^^^^^^^^^^^^^^^^
    "#);
}

#[test]
fn error_nodes_skip_validation() {
    let input = indoc! {r#"
        Q = (ERROR) @err
    "#};
    Query::expect_valid_linking(input, &LANG);
}

#[test]
fn missing_nodes_skip_validation() {
    let input = indoc! {r#"
        Q = (MISSING) @miss
    "#};
    Query::expect_valid_linking(input, &LANG);
}

#[test]
fn multiple_errors_in_query() {
    let input = indoc! {r#"
        Q = (function_declaraton
            nme: (identifer) @name) @fn
    "#};

    let res = Query::expect_invalid_linking(input, &LANG);

    insta::assert_snapshot!(res, @r"
    error: `function_declaraton` is not a valid node type
      |
    1 | Q = (function_declaraton
      |      ^^^^^^^^^^^^^^^^^^^
      |
    help: did you mean `function_declaration`?

    error: `nme` is not a valid field
      |
    2 |     nme: (identifer) @name) @fn
      |     ^^^
      |
    help: did you mean `name`?

    error: `identifer` is not a valid node type
      |
    2 |     nme: (identifer) @name) @fn
      |           ^^^^^^^^^
      |
    help: did you mean `identifier`?
    ");
}

#[test]
fn nested_field_validation() {
    let input = indoc! {r#"
        Q = (function_declaration
            body: (statement_block
                (return_statement) @ret) @body) @fn
    "#};

    Query::expect_valid_linking(input, &LANG);
}

#[test]
fn alternation_with_link_errors() {
    let input = indoc! {r#"
        Q = [(function_declaraton)
         (class_declaraton)] @decl
    "#};

    let res = Query::expect_invalid_linking(input, &LANG);

    insta::assert_snapshot!(res, @r"
    error: `function_declaraton` is not a valid node type
      |
    1 | Q = [(function_declaraton)
      |       ^^^^^^^^^^^^^^^^^^^
      |
    help: did you mean `function_declaration`?

    error: `class_declaraton` is not a valid node type
      |
    2 |  (class_declaraton)] @decl
      |   ^^^^^^^^^^^^^^^^
      |
    help: did you mean `class_declaration`?
    ");
}

#[test]
fn quantified_expr_validation() {
    let input = indoc! {r#"
        Q = (statement_block
            (function_declaration)+ @fns) @block
    "#};

    Query::expect_valid_linking(input, &LANG);
}

#[test]
fn wildcard_node_skips_validation() {
    let input = indoc! {r#"
        Q = (_) @any
    "#};

    Query::expect_valid_linking(input, &LANG);
}

#[test]
fn def_reference_with_link() {
    // Test linking with definition reference as scalar list (no internal captures)
    let input = indoc! {r#"
        Func = (function_declaration)
        Q = (program (Func)+ @funcs)
    "#};

    Query::expect_valid_linking(input, &LANG);
}

#[test]
fn field_on_node_without_fields() {
    let input = indoc! {r#"
        Q = (identifier
            name: (identifier) @inner) @id
    "#};

    let res = Query::expect_invalid_linking(input, &LANG);

    insta::assert_snapshot!(res, @r"
    error: field `name` is not valid on this node type
      |
    1 | Q = (identifier
      |      ---------- on `identifier`
    2 |     name: (identifier) @inner) @id
      |     ^^^^
      |
    help: `identifier` has no fields
    ");
}

#[test]
fn valid_child_via_supertype() {
    let input = indoc! {r#"
        Q = (statement_block
            (function_declaration)) @block
    "#};

    Query::expect_valid_linking(input, &LANG);
}

#[test]
fn valid_child_via_nested_supertype() {
    let input = indoc! {r#"
        Q = (program
            (function_declaration)) @prog
    "#};

    Query::expect_valid_linking(input, &LANG);
}

#[test]
fn deeply_nested_sequences_valid() {
    let input = indoc! {r#"
        Q = (statement_block {{{(function_declaration)}}}) @block
    "#};

    Query::expect_valid_linking(input, &LANG);
}

#[test]
fn deeply_nested_alternations_in_field_valid() {
    let input = indoc! {r#"
        Q = (function_declaration name: [[[(identifier)]]]) @fn
    "#};

    Query::expect_valid_linking(input, &LANG);
}

#[test]
fn ref_followed_valid_case() {
    let input = indoc! {r#"
        Foo = (identifier)
        Q = (function_declaration name: (Foo))
    "#};

    Query::expect_valid_linking(input, &LANG);
}
