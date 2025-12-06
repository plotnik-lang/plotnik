use crate::Query;

#[test]
fn valid_query_with_field() {
    let mut query = Query::try_from("(function_declaration name: (identifier) @name) @fn").unwrap();
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
    let mut query = Query::try_from("(function_declaraton) @fn").unwrap();
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
    let mut query = Query::try_from("(xyzzy_foobar_baz) @fn").unwrap();
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
    let mut query = Query::try_from("(function_declaration nme: (identifier) @name) @fn").unwrap();
    query.link(&plotnik_langs::javascript());
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `nme` is not a valid field
      |
    1 | (function_declaration nme: (identifier) @name) @fn
      |                       ^^^
      |
    help: did you mean `name`?
    ");
}

#[test]
fn unknown_field_no_suggestion() {
    let mut query =
        Query::try_from("(function_declaration xyzzy: (identifier) @name) @fn").unwrap();
    query.link(&plotnik_langs::javascript());
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `xyzzy` is not a valid field
      |
    1 | (function_declaration xyzzy: (identifier) @name) @fn
      |                       ^^^^^
    ");
}

#[test]
fn field_not_on_node_type() {
    let mut query =
        Query::try_from("(function_declaration condition: (identifier) @name) @fn").unwrap();
    query.link(&plotnik_langs::javascript());
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: field `condition` is not valid on this node type
      |
    1 | (function_declaration condition: (identifier) @name) @fn
      |                       ^^^^^^^^^
      |
    help: valid fields for `function_declaration`: `body`, `name`, `parameters`
    ");
}

#[test]
fn negated_field_unknown() {
    let mut query = Query::try_from("(function_declaration !nme) @fn").unwrap();
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
    let mut query = Query::try_from("(function_declaration !condition) @fn").unwrap();
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
fn negated_field_valid() {
    let mut query = Query::try_from("(function_declaration !name) @fn").unwrap();
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
    let mut query = Query::try_from("(function_declaration \"xyzzy_fake_token\") @fn").unwrap();
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
    let mut query = Query::try_from("(ERROR) @err").unwrap();
    query.link(&plotnik_langs::javascript());
    assert!(query.is_valid());

    let mut query2 = Query::try_from("(MISSING) @miss").unwrap();
    query2.link(&plotnik_langs::javascript());
    assert!(query2.is_valid());
}

#[test]
fn multiple_errors_in_query() {
    let mut query = Query::try_from("(function_declaraton nme: (identifer) @name) @fn").unwrap();
    query.link(&plotnik_langs::javascript());
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `function_declaraton` is not a valid node type
      |
    1 | (function_declaraton nme: (identifer) @name) @fn
      |  ^^^^^^^^^^^^^^^^^^^
      |
    help: did you mean `function_declaration`?

    error: `nme` is not a valid field
      |
    1 | (function_declaraton nme: (identifer) @name) @fn
      |                      ^^^
      |
    help: did you mean `name`?

    error: `identifer` is not a valid node type
      |
    1 | (function_declaraton nme: (identifer) @name) @fn
      |                            ^^^^^^^^^
      |
    help: did you mean `identifier`?
    ");
}

#[test]
fn nested_field_validation() {
    let mut query = Query::try_from(
        "(function_declaration body: (statement_block (return_statement) @ret) @body) @fn",
    )
    .unwrap();
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
    let mut query =
        Query::try_from("(function_declaration name: (statement_block) @name) @fn").unwrap();
    query.link(&plotnik_langs::javascript());
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: node type `statement_block` is not valid for this field
      |
    1 | (function_declaration name: (statement_block) @name) @fn
      |                             ^^^^^^^^^^^^^^^^^
      |
    help: valid types for `name`: `identifier`
    ");
}

#[test]
fn alternation_with_link_errors() {
    let mut query = Query::try_from("[(function_declaraton) (class_declaraton)] @decl").unwrap();
    query.link(&plotnik_langs::javascript());
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `function_declaraton` is not a valid node type
      |
    1 | [(function_declaraton) (class_declaraton)] @decl
      |   ^^^^^^^^^^^^^^^^^^^
      |
    help: did you mean `function_declaration`?

    error: `class_declaraton` is not a valid node type
      |
    1 | [(function_declaraton) (class_declaraton)] @decl
      |                         ^^^^^^^^^^^^^^^^
      |
    help: did you mean `class_declaration`?
    ");
}

#[test]
fn sequence_with_link_errors() {
    let mut query =
        Query::try_from("(function_declaration {(identifer) (statement_block)} @body) @fn")
            .unwrap();
    query.link(&plotnik_langs::javascript());
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `identifer` is not a valid node type
      |
    1 | (function_declaration {(identifer) (statement_block)} @body) @fn
      |                         ^^^^^^^^^
      |
    help: did you mean `identifier`?
    ");
}

#[test]
fn quantified_expr_validation() {
    let mut query = Query::try_from("(function_declaration (identifier)+ @names) @fn").unwrap();
    query.link(&plotnik_langs::javascript());
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        CapturedExpr @fn
          NamedNode function_declaration
            CapturedExpr @names
              QuantifiedExpr +
                NamedNode identifier
    ");
}

#[test]
fn wildcard_node_skips_validation() {
    let mut query = Query::try_from("(_) @any").unwrap();
    query.link(&plotnik_langs::javascript());
    assert!(query.is_valid());
}

#[test]
fn def_reference_with_link() {
    let mut query = Query::try_from(
        r#"
        Func = (function_declaration name: (identifier) @name) @fn
        (program (Func)+)
        "#,
    )
    .unwrap();
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
    let mut query = Query::try_from("(identifier name: (identifier) @inner) @id").unwrap();
    query.link(&plotnik_langs::javascript());
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: field `name` is not valid on this node type
      |
    1 | (identifier name: (identifier) @inner) @id
      |             ^^^^
      |
    help: `identifier` has no fields
    ");
}
