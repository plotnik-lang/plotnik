use crate::Query;
use indoc::indoc;

#[test]
fn valid_query_with_field() {
    let input = indoc! {r#"
        (function_declaration
            name: (identifier) @name) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        CapturedExpr @fn
          NamedNode function_declaration
            CapturedExpr @name
              FieldExpr name:
                NamedNode identifier
    ");
}

#[test]
fn unknown_node_type_with_suggestion() {
    let input = indoc! {r#"
        (function_declaraton) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `function_declaraton` is not a valid node type
      |
    1 | (function_declaraton) @fn
      |  ^^^^^^^^^^^^^^^^^^^
      |
    help: did you mean `function_declaration`?
    ");
}

#[test]
fn unknown_node_type_no_suggestion() {
    let input = indoc! {r#"
        (xyzzy_foobar_baz) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `xyzzy_foobar_baz` is not a valid node type
      |
    1 | (xyzzy_foobar_baz) @fn
      |  ^^^^^^^^^^^^^^^^
    ");
}

#[test]
fn unknown_field_with_suggestion() {
    let input = indoc! {r#"
        (function_declaration
            nme: (identifier) @name) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
        (function_declaration
            xyzzy: (identifier) @name) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `xyzzy` is not a valid field
      |
    2 |     xyzzy: (identifier) @name) @fn
      |     ^^^^^
    ");
}

#[test]
fn field_not_on_node_type() {
    let input = indoc! {r#"
        (function_declaration
            condition: (identifier) @name) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: field `condition` is not valid on this node type
      |
    2 |     condition: (identifier) @name) @fn
      |     ^^^^^^^^^
      |
    help: valid fields for `function_declaration`: `body`, `name`, `parameters`
    ");
}

#[test]
fn field_not_on_node_type_with_suggestion() {
    let input = indoc! {r#"
        (function_declaration
            parameter: (formal_parameters) @params) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::typescript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: field `parameter` is not valid on this node type
      |
    2 |     parameter: (formal_parameters) @params) @fn
      |     ^^^^^^^^^
      |
    help: did you mean `parameters`?
    help: valid fields for `function_declaration`: `body`, `name`, `parameters`, `return_type`, `type_parameters`
    ");
}

#[test]
fn negated_field_unknown() {
    let input = indoc! {r#"
        (function_declaration !nme) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `nme` is not a valid field
      |
    1 | (function_declaration !nme) @fn
      |                        ^^^
      |
    help: did you mean `name`?
    ");
}

#[test]
fn negated_field_not_on_node_type() {
    let input = indoc! {r#"
        (function_declaration !condition) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: field `condition` is not valid on this node type
      |
    1 | (function_declaration !condition) @fn
      |                        ^^^^^^^^^
      |
    help: valid fields for `function_declaration`: `body`, `name`, `parameters`
    ");
}

#[test]
fn negated_field_not_on_node_type_with_suggestion() {
    let input = indoc! {r#"
        (function_declaration !parameter) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::typescript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: field `parameter` is not valid on this node type
      |
    1 | (function_declaration !parameter) @fn
      |                        ^^^^^^^^^
      |
    help: did you mean `parameters`?
    help: valid fields for `function_declaration`: `body`, `name`, `parameters`, `return_type`, `type_parameters`
    ");
}

#[test]
fn negated_field_valid() {
    let input = indoc! {r#"
        (function_declaration !name) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        CapturedExpr @fn
          NamedNode function_declaration
            NegatedField !name
    ");
}

#[test]
fn anonymous_node_unknown() {
    let input = indoc! {r#"
        (function_declaration "xyzzy_fake_token") @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: `xyzzy_fake_token` is not a valid node type
      |
    1 | (function_declaration "xyzzy_fake_token") @fn
      |                        ^^^^^^^^^^^^^^^^
    "#);
}

#[test]
fn error_and_missing_nodes_skip_validation() {
    let input = indoc! {r#"
        (ERROR) @err
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(query.is_valid());

    let input2 = indoc! {r#"
        (MISSING) @miss
    "#};

    let mut query2 = Query::try_from(input2).unwrap();
    query2.link(&plotnik_langs::javascript());

    assert!(query2.is_valid());
}

#[test]
fn multiple_errors_in_query() {
    let input = indoc! {r#"
        (function_declaraton
            nme: (identifer) @name) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `function_declaraton` is not a valid node type
      |
    1 | (function_declaraton
      |  ^^^^^^^^^^^^^^^^^^^
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
        (function_declaration
            body: (statement_block
                (return_statement) @ret) @body) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        CapturedExpr @fn
          NamedNode function_declaration
            CapturedExpr @body
              FieldExpr body:
                NamedNode statement_block
                  CapturedExpr @ret
                    NamedNode return_statement
    ");
}

#[test]
fn invalid_child_type_for_field() {
    let input = indoc! {r#"
        (function_declaration
            name: (statement_block) @name) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: node type `statement_block` is not valid for this field
      |
    2 |     name: (statement_block) @name) @fn
      |           ^^^^^^^^^^^^^^^^^
      |
    help: valid types for `name`: `identifier`
    ");
}

#[test]
fn alternation_with_link_errors() {
    let input = indoc! {r#"
        [(function_declaraton)
         (class_declaraton)] @decl
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `function_declaraton` is not a valid node type
      |
    1 | [(function_declaraton)
      |   ^^^^^^^^^^^^^^^^^^^
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
fn sequence_with_link_errors() {
    let input = indoc! {r#"
        (function_declaration
            {(identifer)
             (statement_block)} @body) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `identifer` is not a valid node type
      |
    2 |     {(identifer)
      |       ^^^^^^^^^
      |
    help: did you mean `identifier`?

    error: `statement_block` cannot be a child of this node
      |
    3 |      (statement_block)} @body) @fn
      |      ^^^^^^^^^^^^^^^^^
      |
    help: `function_declaration` only accepts children via fields
    ");
}

#[test]
fn quantified_expr_validation() {
    let input = indoc! {r#"
        (statement_block
            (function_declaration)+ @fns) @block
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        CapturedExpr @block
          NamedNode statement_block
            CapturedExpr @fns
              QuantifiedExpr +
                NamedNode function_declaration
    ");
}

#[test]
fn wildcard_node_skips_validation() {
    let input = indoc! {r#"
        (_) @any
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(query.is_valid());
}

#[test]
fn def_reference_with_link() {
    let input = indoc! {r#"
        Func = (function_declaration
            name: (identifier) @name) @fn
        (program (Func)+)
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def Func
        CapturedExpr @fn
          NamedNode function_declaration
            CapturedExpr @name
              FieldExpr name:
                NamedNode identifier
      Def
        NamedNode program
          QuantifiedExpr +
            Ref Func
    ");
}

#[test]
fn field_on_node_without_fields() {
    let input = indoc! {r#"
        (identifier
            name: (identifier) @inner) @id
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: field `name` is not valid on this node type
      |
    2 |     name: (identifier) @inner) @id
      |     ^^^^
      |
    help: `identifier` has no fields
    ");
}

#[test]
fn invalid_child_type_no_children_allowed() {
    let input = indoc! {r#"
        (function_declaration
            (class_declaration)) @fn
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `class_declaration` cannot be a child of this node
      |
    2 |     (class_declaration)) @fn
      |     ^^^^^^^^^^^^^^^^^^^
      |
    help: `function_declaration` only accepts children via fields
    ");
}

#[test]
fn invalid_child_type_wrong_type() {
    let input = indoc! {r#"
        (statement_block
            (identifier)) @block
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `identifier` cannot be a child of this node
      |
    2 |     (identifier)) @block
      |     ^^^^^^^^^^^^
      |
    help: valid children for `statement_block`: `statement`
    ");
}

#[test]
fn valid_child_via_supertype() {
    let input = indoc! {r#"
        (statement_block
            (function_declaration)) @block
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(query.is_valid());
}

#[test]
fn valid_child_via_nested_supertype() {
    let input = indoc! {r#"
        (program
            (function_declaration)) @prog
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(query.is_valid());
}

#[test]
fn invalid_anonymous_child() {
    let input = indoc! {r#"
        (statement_block
            "function") @block
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: `function` cannot be a child of this node
      |
    2 |     "function") @block
      |     ^^^^^^^^^^
      |
    help: valid children for `statement_block`: `statement`
    "#);
}

#[test]
fn invalid_child_in_alternation() {
    let input = indoc! {r#"
        (statement_block
            [(function_declaration) (identifier)]) @block
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `identifier` cannot be a child of this node
      |
    2 |     [(function_declaration) (identifier)]) @block
      |                             ^^^^^^^^^^^^
      |
    help: valid children for `statement_block`: `statement`
    ");
}

#[test]
fn invalid_child_in_sequence() {
    let input = indoc! {r#"
        (statement_block
            {(function_declaration) (identifier)}) @block
    "#};

    let mut query = Query::try_from(input).unwrap();
    query.link(&plotnik_langs::javascript());

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `identifier` cannot be a child of this node
      |
    2 |     {(function_declaration) (identifier)}) @block
      |                             ^^^^^^^^^^^^
      |
    help: valid children for `statement_block`: `statement`
    ");
}
