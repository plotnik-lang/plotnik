use crate::test_utils::colliding_node_type_grammar;
use crate::{Query, QueryBuilder, SourceMap};
use indoc::indoc;
use plotnik_core::NodeType;

fn assert_links_colliding_node_types(files: &[(&str, &str)]) {
    let grammar = colliding_node_type_grammar();
    let mut source_map = SourceMap::new();
    for (path, content) in files {
        source_map.add_file(path, content);
    }

    let query = QueryBuilder::new(source_map).parse().unwrap().analyze();
    if !query.is_valid() {
        panic!(
            "Expected valid query, got error:\n{}",
            query.dump_diagnostics()
        );
    }

    let query = query.link(&grammar);
    if !query.is_valid() {
        panic!(
            "Expected valid linking, got error:\n{}",
            query.dump_diagnostics()
        );
    }

    let sym = query
        .interner()
        .get("number")
        .expect("linked node name must be interned");
    let named_id = grammar.resolve_named_node("number").unwrap();
    let anonymous_id = grammar.resolve_anonymous_node("number").unwrap();

    assert_ne!(named_id, anonymous_id);
    assert_eq!(
        query.node_type_ids().get(&NodeType::Named(sym)),
        Some(&named_id)
    );
    assert_eq!(
        query.node_type_ids().get(&NodeType::Anonymous(sym)),
        Some(&anonymous_id)
    );
}

#[test]
fn predicate_on_non_leaf() {
    let input = r"Q = (function_declaration == 'foo')";

    let res = Query::expect_invalid_linking(input);

    insta::assert_snapshot!(res, @r"
    error: predicates match text content, but this node can contain children
      |
    1 | Q = (function_declaration == 'foo')
      |                           ^^^^^^^^
    ");
}

#[test]
fn predicate_on_leaf_valid() {
    let input = r#"Q = (identifier == "foo")"#;
    Query::expect_valid_linking(input);
}

#[test]
fn resolves_named_and_anonymous_node_types_with_same_name() {
    assert_links_colliding_node_types(&[
        ("named.ptk", "A = (number)"),
        ("anonymous.ptk", "Q = \"number\""),
    ]);
    assert_links_colliding_node_types(&[
        ("anonymous.ptk", "Q = \"number\""),
        ("named.ptk", "A = (number)"),
    ]);
}

#[test]
fn valid_query_with_field() {
    let input = indoc! {r#"
        Q = (function_declaration
            name: (identifier) @name) @fn
    "#};
    Query::expect_valid_linking(input);
}

#[test]
fn unknown_node_type_with_suggestion() {
    let input = indoc! {r#"
        Q = (function_declaraton) @fn
    "#};

    let res = Query::expect_invalid_linking(input);

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

    let res = Query::expect_invalid_linking(input);

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

    let res = Query::expect_invalid_linking(input);

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

    let res = Query::expect_invalid_linking(input);

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

    let res = Query::expect_invalid_linking(input);

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

    let res = Query::expect_invalid_linking(input);

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
        Q = (function_declaration -nme) @fn
    "#};

    let res = Query::expect_invalid_linking(input);

    insta::assert_snapshot!(res, @r"
    error: `nme` is not a valid field
      |
    1 | Q = (function_declaration -nme) @fn
      |                            ^^^
      |
    help: did you mean `name`?
    ");
}

#[test]
fn negated_field_not_on_node_type() {
    let input = indoc! {r#"
        Q = (function_declaration -condition) @fn
    "#};

    let res = Query::expect_invalid_linking(input);

    insta::assert_snapshot!(res, @r"
    error: field `condition` is not valid on this node type
      |
    1 | Q = (function_declaration -condition) @fn
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
        Q = (function_declaration -parameter) @fn
    "#};

    let res = Query::expect_invalid_linking(input);

    insta::assert_snapshot!(res, @r"
    error: field `parameter` is not valid on this node type
      |
    1 | Q = (function_declaration -parameter) @fn
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
    // `label` is optional on `break_statement`, so asserting its absence is satisfiable.
    let input = indoc! {r#"
        Q = (break_statement -label) @brk
    "#};

    Query::expect_valid_linking(input);
}

#[test]
fn anonymous_node_unknown() {
    let input = indoc! {r#"
        Q = (function_declaration "xyzzy_fake_token") @fn
    "#};

    let res = Query::expect_invalid_linking(input);

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
    Query::expect_valid_linking(input);
}

#[test]
fn missing_nodes_skip_validation() {
    let input = indoc! {r#"
        Q = (MISSING) @miss
    "#};
    Query::expect_valid_linking(input);
}

#[test]
fn multiple_errors_in_query() {
    let input = indoc! {r#"
        Q = (function_declaraton
            nme: (identifer) @name) @fn
    "#};

    let res = Query::expect_invalid_linking(input);

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

    Query::expect_valid_linking(input);
}

#[test]
fn alternation_with_link_errors() {
    let input = indoc! {r#"
        Q = [(function_declaraton)
         (class_declaraton)] @decl
    "#};

    let res = Query::expect_invalid_linking(input);

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

    Query::expect_valid_linking(input);
}

#[test]
fn wildcard_node_skips_validation() {
    let input = indoc! {r#"
        Q = (_) @any
    "#};

    Query::expect_valid_linking(input);
}

#[test]
fn def_reference_with_link() {
    // Test linking with definition reference as scalar list (no internal captures)
    let input = indoc! {r#"
        Func = (function_declaration)
        Q = (program (Func)+ @funcs)
    "#};

    Query::expect_valid_linking(input);
}

#[test]
fn field_on_node_without_fields() {
    let input = indoc! {r#"
        Q = (identifier
            name: (identifier) @inner) @id
    "#};

    let res = Query::expect_invalid_linking(input);

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

    Query::expect_valid_linking(input);
}

#[test]
fn valid_child_via_nested_supertype() {
    let input = indoc! {r#"
        Q = (program
            (function_declaration)) @prog
    "#};

    Query::expect_valid_linking(input);
}

#[test]
fn deeply_nested_sequences_valid() {
    let input = indoc! {r#"
        Q = (statement_block {{{(function_declaration)}}}) @block
    "#};

    Query::expect_valid_linking(input);
}

#[test]
fn deeply_nested_alternations_in_field_valid() {
    let input = indoc! {r#"
        Q = (function_declaration name: [[[(identifier)]]]) @fn
    "#};

    Query::expect_valid_linking(input);
}

#[test]
fn ref_followed_valid_case() {
    let input = indoc! {r#"
        Foo = (identifier)
        Q = (function_declaration name: (Foo))
    "#};

    Query::expect_valid_linking(input);
}

#[test]
fn diamond_reference_graph_links() {
    // Each definition references the next one twice, so the reference graph is
    // diamond-shaped. Without memoization, structural validation walks it
    // 2^depth times — intractable by this depth (issue #416); with it, instant.
    let depth = 30;
    let mut input = String::new();
    for i in 0..depth {
        input.push_str(&format!(
            "D{i} = (statement_block (D{next}) (D{next}))\n",
            next = i + 1
        ));
    }
    input.push_str(&format!("D{depth} = (identifier) @x\n"));

    Query::expect_valid_linking(&input);
}

#[test]
fn shared_definition_validated_per_context() {
    // A definition whose body is a bare field is validated against each
    // referencing parent, so memoization must key on context, not name alone:
    // `arguments` is valid on `call_expression` but not on `binary_expression`.
    let input = indoc! {r#"
        Args = arguments: (identifier)
        Q = (statement_block (call_expression (Args)) (binary_expression (Args)))
    "#};

    let res = Query::expect_invalid_linking(input);

    insta::assert_snapshot!(res, @"
    error: field `arguments` is not valid on this node type
      |
    1 | Args = arguments: (identifier)
      |        ^^^^^^^^^
    2 | Q = (statement_block (call_expression (Args)) (binary_expression (Args)))
      |                                                ----------------- on `binary_expression`
      |
    help: valid fields for `binary_expression`: `left`, `operator`, `right`

    error: `identifier` can't be the value of `arguments`
      |
    1 | Args = arguments: (identifier)
      |        ---------   ^^^^^^^^^^
      |        |
      |        field `arguments`
      |
    help: `arguments` accepts: `arguments`, `template_string`

    error: `call_expression` cannot be a child of this node
      |
    2 | Q = (statement_block (call_expression (Args)) (binary_expression (Args)))
      |      ---------------  ^^^^^^^^^^^^^^^
      |      |
      |      on `statement_block`
      |
    help: valid children of `statement_block`: `statement`

    error: `binary_expression` cannot be a child of this node
      |
    2 | Q = (statement_block (call_expression (Args)) (binary_expression (Args)))
      |      --------------- on `statement_block`      ^^^^^^^^^^^^^^^^^
      |
    help: valid children of `statement_block`: `statement`
    ");
}

#[test]
fn invalid_child_kind_rejected() {
    let input = r"Q = (function_declaration (class_declaration))";

    let res = Query::expect_invalid_linking(input);

    insta::assert_snapshot!(res, @"
    error: `class_declaration` cannot be a child of this node
      |
    1 | Q = (function_declaration (class_declaration))
      |      --------------------  ^^^^^^^^^^^^^^^^^
      |      |
      |      on `function_declaration`
      |
    help: `function_declaration` has no unlabeled children — its children are fields: `body: (statement_block)`, `name: (identifier)`, `parameters: (formal_parameters)`
    ");
}

#[test]
fn invalid_field_value_kind_rejected() {
    let input = r"Q = (function_declaration name: (number))";

    let res = Query::expect_invalid_linking(input);

    insta::assert_snapshot!(res, @"
    error: `number` can't be the value of `name`
      |
    1 | Q = (function_declaration name: (number))
      |                           ----   ^^^^^^
      |                           |
      |                           field `name`
      |
    help: `name` accepts: `identifier`
    ");
}

#[test]
fn invalid_subtype_rejected() {
    let input = r"Q = (expression#statement_block)";

    let res = Query::expect_invalid_linking(input);

    insta::assert_snapshot!(res, @"
    error: `statement_block` is not a kind of `expression`
      |
    1 | Q = (expression#statement_block)
      |      ---------- ^^^^^^^^^^^^^^^
      |      |
      |      base type `expression`
      |
    help: kinds of `expression` include: `assignment_expression`, `augmented_assignment_expression`, `await_expression`, `binary_expression`, `jsx_element`, `jsx_self_closing_element`, `new_expression`, `primary_expression`, ... (4 more)
    ");
}

#[test]
fn child_under_leaf_token_rejected() {
    let input = r"Q = (identifier (_))";

    let res = Query::expect_invalid_linking(input);

    insta::assert_snapshot!(res, @r#"
    error: `identifier` is a leaf token — it has no child nodes
      |
    1 | Q = (identifier (_))
      |      ---------- ^^^
      |      |
      |      `identifier`
      |
    help: a leaf token's content is its text — match it directly `(identifier)` or by value `(identifier == "foo")`
    "#);
}

#[test]
fn negated_required_field_rejected() {
    let input = r"Q = (function_declaration -name) @fn";

    let res = Query::expect_invalid_linking(input);

    insta::assert_snapshot!(res, @"
    error: `-name` can never match
      |
    1 | Q = (function_declaration -name) @fn
      |      --------------------  ^^^^
      |      |
      |      on `function_declaration`
      |
    help: `-name` requires `name` to be absent, but every `function_declaration` has one — drop `-name`
    ");
}

#[test]
fn anonymous_only_field_value_rejected() {
    let input = r"Q = (binary_expression operator: (identifier))";

    let res = Query::expect_invalid_linking(input);

    insta::assert_snapshot!(res, @r#"
    error: `identifier` can't be the value of `operator`
      |
    1 | Q = (binary_expression operator: (identifier))
      |                        --------   ^^^^^^^^^^
      |                        |
      |                        field `operator`
      |
    help: `operator` accepts only literal tokens — write `operator: "!="`
    "#);
}

#[test]
fn alternation_branch_admissibility_is_not_checked() {
    // One branch is a valid `body`, so the alternation is satisfiable; structural checks are
    // skipped inside `[...]`.
    let input = r"Q = (function_declaration body: [(statement_block) @a (class_declaration) @b])";

    Query::expect_valid_linking(input);
}

#[test]
fn transitive_supertype_as_child_valid() {
    // `decorator` accepts `call_expression`/`identifier`/`member_expression`; `expression` and
    // `primary_expression` are supertypes that reach them only through multi-hop subtyping.
    Query::expect_valid_linking(r"Q = (decorator (expression))");
    Query::expect_valid_linking(r"Q = (decorator (primary_expression))");
}

#[test]
fn transitive_subtype_valid() {
    // `expression -> primary_expression -> call_expression` is a two-hop subtype path.
    Query::expect_valid_linking(r"Q = (expression#call_expression)");
}

#[test]
fn overlapping_supertype_refinement_valid() {
    // `expression` and `pattern` are sibling supertypes that share concrete members (`identifier`,
    // `member_expression`, ...), so a single node can be both. `(expression#pattern)` can therefore
    // match, even though `pattern` is not itself listed among `expression`'s subtypes.
    Query::expect_valid_linking(r"Q = (expression#pattern)");
    Query::expect_valid_linking(r"Q = (pattern#expression)");
}

#[test]
fn quantified_child_admissibility_is_not_checked() {
    // `(identifier)?` matches zero, so the position is satisfiable; structural checks are skipped
    // inside quantifiers.
    let input = r"Q = (statement_block (identifier)?)";

    Query::expect_valid_linking(input);
}

#[test]
fn child_under_childless_syntax_node_valid() {
    // `debugger_statement` is childless in node-types but is a syntax node, not a leaf token:
    // an extra (comment) can attach, so `(_)` is satisfiable and must not be rejected.
    Query::expect_valid_linking(r"Q = (debugger_statement (_))");
}

#[test]
fn violation_in_alternation_branch_does_not_reject_query() {
    // A branch with an internal violation is a dead branch, but a sibling branch keeps the
    // alternation satisfiable — so the whole query can match and must NOT be rejected. One case per
    // violation kind, each paired with a valid `(identifier)` branch.
    Query::expect_valid_linking(r"Q = [(function_declaration (class_declaration)) (identifier)]");
    Query::expect_valid_linking(r"Q = [(function_declaration name: (number)) (identifier)]");
    Query::expect_valid_linking(r"Q = [(expression#statement_block) (identifier)]");
    Query::expect_valid_linking(r"Q = [(identifier (number)) (number)]");
    Query::expect_valid_linking(r"Q = [(function_declaration -name) (identifier)]");
    Query::expect_valid_linking(
        r"Q = [(function_declaration condition: (identifier)) (identifier)]",
    );
    Query::expect_valid_linking(r"Q = [(function_declaration -condition) (identifier)]");
    Query::expect_valid_linking(r#"Q = [(function_declaration == "x") (identifier)]"#);
}

#[test]
fn violation_under_quantifier_does_not_reject_query() {
    // A quantified subtree matches zero times, so an internal violation never blocks a match.
    Query::expect_valid_linking(r"Q = (program (function_declaration (class_declaration))* @x)");
    Query::expect_valid_linking(r"Q = (program (function_declaration name: (number))? @x)");
    Query::expect_valid_linking(r"Q = (program (function_declaration -name)? @x)");
    Query::expect_valid_linking(
        r"Q = (program (function_declaration condition: (identifier))? @x)",
    );
    Query::expect_valid_linking(r"Q = (program (function_declaration -condition)? @x)");
    Query::expect_valid_linking(r#"Q = (program (function_declaration == "x")? @x)"#);
    // The same holds at arbitrary depth and through `+`.
    Query::expect_valid_linking(
        r"Q = (program (function_declaration name: (number) body: (number))+ @x)",
    );
}

#[test]
fn deep_violation_in_alternation_does_not_reject_query() {
    // Skipping applies at every level nested under the alternation, not just the first.
    Query::expect_valid_linking(
        r"Q = [(call_expression function: (member_expression object: (number))) (identifier)]",
    );
}

#[test]
fn sibling_of_alternation_is_still_checked() {
    // Guards against over-skipping: only the alternation is skipped. A bad bare child sitting
    // beside it is always evaluated and must be rejected.
    Query::expect_invalid_linking(
        r"Q = (function_declaration (class_declaration) [(identifier) (number)])",
    );
}

#[test]
fn shared_definition_rejected_at_non_deferred_use_after_deferred() {
    // `Bad` is a bare field, impossible only against a parent lacking it — standalone (ctx = None)
    // it is not checked, so only its use carries the rejection. `arguments` is absent on
    // `binary_expression`. `Bad` is reached first inside the alternation (deferred — field check
    // skipped, result memoized) and then as a direct child (non-deferred). The memo keys on
    // `deferred`, so the cached deferred result does not mask the rejection at the non-deferred use
    // — keying on `(name, ctx)` alone would wrongly accept this query.
    let input = indoc! {r#"
        Bad = arguments: (identifier)
        Q = (binary_expression [(Bad) (identifier)] (Bad))
    "#};

    Query::expect_invalid_linking(input);
}
