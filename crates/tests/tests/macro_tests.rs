use plotnik::LimitExceeded;
use plotnik::tree_sitter::{Language, Parser, Tree};

mod support;

fn parse(language: &Language, source: &str) -> Tree {
    let mut parser = Parser::new();
    parser.set_language(language).expect("language loads");
    parser.parse(source, None).expect("source parses")
}

fn js() -> Language {
    tree_sitter_javascript::LANGUAGE.into()
}

mod queries {
    plotnik::query! {
        "Idents = (program (expression_statement (identifier) @id))",
        grammar = "tree-sitter-javascript",
    }

    plotnik::query! {
        "Probe = {(program)}",
        grammar = "tree-sitter-javascript",
    }
}

mod tree_sitter_package {
    plotnik::query! {
        "Q = (program (expression_statement (identifier) @id))",
        grammar = "tree-sitter-javascript",
    }
}

mod tsx {
    plotnik::query! {
        "Q = (program (expression_statement (identifier) @id))",
        grammar = "tree-sitter-typescript/tsx",
    }
}

mod from_file {
    plotnik::query! {
        grammar = "tree-sitter-javascript",
        file = "macro_queries/idents.ptk",
    }
}

mod depth_limited {
    plotnik::query! {
        r#"
        Expr = [
          Id: (identifier) @id
          Paren: (parenthesized_expression (Expr) @expr)
        ]
        Q = (program (expression_statement (Expr) @expr))
        "#,
        grammar = "tree-sitter-javascript",
        depth = 1,
    }
}

mod nodes {
    plotnik::query! {
        r#"
        Id = (identifier) @text :: text
        Probe = (identifier)
        Token = "+"
        Binary = [
          First: (binary_expression left: (_) @left)
          Second: (binary_expression right: (_) @right)
        ]
        Args = (arguments (identifier)* @ids :: text)
        Calls = [
          (call_expression
            function: (identifier) @bad :: text
            arguments: (arguments (number == "0"))
          )
          (call_expression
            function: (identifier != "skip") @name :: text
            arguments: (Args) @args
          )
        ]
        Left = left: (identifier) @node
        Pair = (binary_expression (Left) @left right: (identifier) @text :: text)
        "#,
        grammar = "tree-sitter-javascript",
    }
}

mod skew {
    plotnik::query! {
        "Q = (program (expression_statement (identifier) @id))",
        grammar = "tree-sitter-javascript",
    }
}

#[test]
fn facade_executes_generated_types_from_multiple_expansions() {
    let source = "x;";
    let tree = parse(&js(), source);

    let value = plotnik::exec::<queries::Idents>(tree.root_node(), source)
        .expect("execution succeeds")
        .expect("matches");
    let json =
        serde_json::to_string(&plotnik::rt::WithSource::new(&value, source)).expect("serializes");

    assert_eq!(value.id.utf8_text(source.as_bytes()), Ok("x"));
    assert_eq!(
        plotnik::find::<queries::Idents>(tree.root_node(), source),
        Ok(Some(value.clone()))
    );
    assert_eq!(
        plotnik::find_all::<queries::Idents>(tree.root_node(), source),
        Ok(vec![value.clone()])
    );
    assert_eq!(
        plotnik::each::<queries::Idents>(tree.root_node(), source).collect::<Result<Vec<_>, _>>(),
        Ok(vec![value])
    );
    assert_eq!(
        plotnik::exec::<queries::Probe>(tree.root_node(), source),
        Ok(Some(tree.root_node()))
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).expect("valid JSON"),
        serde_json::json!({"id": {"kind": "identifier", "span": [0, 1], "text": "x"}})
    );
}

#[test]
fn grammar_packages_and_file_form_resolve() {
    let source = "x;";
    let javascript_tree = parse(&tree_sitter_javascript::LANGUAGE.into(), source);
    let tsx_tree = parse(&tree_sitter_typescript::LANGUAGE_TSX.into(), source);

    assert_eq!(
        from_file::Q::exec(javascript_tree.root_node(), source)
            .unwrap()
            .unwrap()
            .id
            .utf8_text(source.as_bytes()),
        Ok("x")
    );
    assert_eq!(
        tree_sitter_package::Q::exec(javascript_tree.root_node(), source)
            .unwrap()
            .unwrap()
            .id
            .utf8_text(source.as_bytes()),
        Ok("x")
    );
    assert_eq!(
        tsx::Q::exec(tsx_tree.root_node(), source)
            .unwrap()
            .unwrap()
            .id
            .utf8_text(source.as_bytes()),
        Ok("x")
    );
}

#[test]
fn decode_depth_limit_terminates_execution_and_search() {
    let source = "((x));";
    let tree = parse(&js(), source);

    assert!(matches!(
        depth_limited::Q::exec(tree.root_node(), source),
        Err(LimitExceeded::DecodeDepth(1))
    ));
    let mut each = depth_limited::Q::each(tree.root_node(), source);
    assert_eq!(each.next(), Some(Err(LimitExceeded::DecodeDepth(1))));
    assert_eq!(each.next(), None);
    assert_eq!(each.next(), None);
    assert_eq!(
        depth_limited::Q::find(tree.root_node(), source),
        Err(LimitExceeded::DecodeDepth(1))
    );
    assert_eq!(
        depth_limited::Q::find_all(tree.root_node(), source),
        Err(LimitExceeded::DecodeDepth(1))
    );

    let source = "x; ((y));";
    let tree = parse(&js(), source);
    let mut each = depth_limited::Expr::each(tree.root_node(), source);
    let first = each.next().unwrap().unwrap();
    assert_eq!(each.next(), Some(Err(LimitExceeded::DecodeDepth(1))));
    assert_eq!(each.next(), None);
    drop(each);

    let depth_limited::Expr::Id { id } = first else {
        panic!("the first candidate is an identifier");
    };
    assert_eq!(id.byte_range(), 0..1);
    assert_eq!(id.utf8_text(source.as_bytes()), Ok("x"));
}

#[test]
fn suitable_nonroot_application_and_context_composition() {
    let source = "alpha + beta; gamma;";
    let tree = parse(&js(), source);
    let binary = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .named_child(0)
        .unwrap();
    let alpha = binary.child_by_field_name("left").unwrap();

    assert_eq!(
        nodes::Id::exec(alpha, source).unwrap().unwrap().text,
        "alpha"
    );
    assert_eq!(nodes::Probe::exec(alpha, source), Ok(Some(alpha)));
    assert_eq!(nodes::Id::exec(binary, source), Ok(None));
    assert_eq!(nodes::Id::exec(tree.root_node(), source), Ok(None));
    assert_eq!(
        nodes::Probe::find_all(binary, source)
            .unwrap()
            .iter()
            .map(|node| node.utf8_text(source.as_bytes()).unwrap())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta"]
    );
    let pair = nodes::Pair::exec(binary, source).unwrap().unwrap();
    assert_eq!(pair.left.node, alpha);
    assert_eq!(pair.text, "beta");
    let token = nodes::Token::find(binary, source).unwrap().unwrap();
    assert!(!token.is_named());
    assert_eq!(token.utf8_text(source.as_bytes()), Ok("+"));
    assert_eq!(nodes::Token::find_all(alpha, source), Ok(vec![]));
    assert_eq!(nodes::Token::find(alpha, source), Ok(None));
    assert_eq!(nodes::Token::each(alpha, source).next(), None);
}

#[test]
fn search_returns_outermost_matches_in_preorder_with_one_preferred_result() {
    let source = "alpha + (beta * gamma); delta - epsilon;";
    let tree = parse(&js(), source);
    let scope = tree.root_node();

    let values = nodes::Binary::each(scope, source)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let lefts = values
        .iter()
        .map(|value| match value {
            nodes::Binary::First { left } => left.utf8_text(source.as_bytes()).unwrap(),
            nodes::Binary::Second { .. } => panic!("source-order preference selects First"),
        })
        .collect::<Vec<_>>();

    assert_eq!(lefts, vec!["alpha", "delta"]);
    assert_eq!(
        nodes::Binary::find(scope, source),
        Ok(Some(values[0].clone()))
    );
    assert_eq!(nodes::Binary::find_all(scope, source), Ok(values.clone()));
    let outer = scope.named_child(0).unwrap().named_child(0).unwrap();
    assert_eq!(
        nodes::Binary::exec(outer, source),
        Ok(Some(values[0].clone()))
    );
    assert_eq!(
        nodes::Binary::find_all(outer, source),
        Ok(values[..1].to_vec())
    );
}

#[test]
fn quantifier_lists_stay_inside_independent_results() {
    let source = "f(alpha, beta); g(gamma);";
    let tree = parse(&js(), source);

    let mut each = nodes::Args::each(tree.root_node(), source);
    let first = each.next().unwrap().unwrap();
    let second = each.next().unwrap().unwrap();
    assert_eq!(each.next(), None);
    drop(each);

    assert_eq!(first.ids, vec!["alpha", "beta"]);
    assert_eq!(second.ids, vec!["gamma"]);
    assert_eq!(
        nodes::Args::find_all(tree.root_node(), source),
        Ok(vec![first, second])
    );
}

#[test]
fn failed_captures_do_not_leak_into_later_nested_results() {
    let source = "f(alpha, beta); skip(1); skip(g(gamma)); h(delta);";
    let tree = parse(&js(), source);

    let mut each = nodes::Calls::each(tree.root_node(), source);
    let first = each.next().unwrap().unwrap();
    let second = each.next().unwrap().unwrap();
    let third = each.next().unwrap().unwrap();
    assert_eq!(each.next(), None);
    drop(each);

    let json = serde_json::to_value(plotnik::rt::WithSource::new(
        &vec![first, second, third],
        source,
    ))
    .expect("retained values serialize after iterator drop");
    assert_eq!(
        json,
        serde_json::json!([
            {"bad": null, "name": "f", "args": {"ids": ["alpha", "beta"]}},
            {"bad": null, "name": "g", "args": {"ids": ["gamma"]}},
            {"bad": null, "name": "h", "args": {"ids": ["delta"]}}
        ])
    );
}

#[test]
fn vm_nested_results_survive_resume_terminal_error_and_drop() {
    use plotnik_compiler::{BytecodeConfig, Limit, QueryBuilder, RuntimeLimitSpec, VM};

    let query = indoc::indoc! {r#"
        Q = [
          (call_expression
            function: (identifier) @bad :: text
            arguments: (arguments (number == "0"))
          )
          (call_expression
            function: (identifier != "skip") @name
            arguments: (arguments (identifier)* @args :: text)
          )
        ]
    "#};
    let compiled = QueryBuilder::from_inline(query)
        .compile(support::javascript_grammar())
        .expect("query compiles");
    let module = compiled
        .emit(BytecodeConfig::new())
        .expect("query emits")
        .into_artifact()
        .expect("query is valid");
    let source = format!(
        "f(alpha, beta); skip(1); g(gamma); {}",
        "later;".repeat(100)
    );
    let tree = parse(&js(), &source);

    let mut each = VM::builder(&source, &tree)
        .limits(RuntimeLimitSpec {
            fuel_limit: Limit::Of(250),
            ..Default::default()
        })
        .build()
        .traverse(&module, module.entrypoint("Q").unwrap());
    let first = each.next().unwrap().unwrap();
    let second = each.next().unwrap().unwrap();
    assert_eq!(each.next(), Some(Err(LimitExceeded::OutOfFuel(250))));
    assert_eq!(each.next(), None);
    assert_eq!(each.next(), None);
    drop(each);

    assert_eq!(
        serde_json::to_value(vec![first, second]).unwrap(),
        serde_json::json!([
            {"bad": null, "name": {"kind": "identifier", "text": "f", "span": [0, 1]},
             "args": ["alpha", "beta"]},
            {"bad": null, "name": {"kind": "identifier", "text": "g", "span": [25, 26]},
             "args": ["gamma"]}
        ])
    );
}

#[test]
fn result_borrows_follow_payload_not_iterator_scratch() {
    let source = String::from("alpha + beta;");
    let text = {
        let tree = parse(&js(), &source);
        nodes::Id::find(tree.root_node(), &source).unwrap().unwrap()
    };
    assert_eq!(text.text, "alpha");

    let tree = parse(&js(), &source);
    let node = {
        let temporary_source = source.clone();
        nodes::Probe::find(tree.root_node(), &temporary_source)
            .unwrap()
            .unwrap()
    };
    assert_eq!(node.byte_range(), 0..5);

    let mut each = nodes::Pair::each(tree.root_node(), &source);
    let pair = each.next().unwrap().unwrap();
    assert_eq!(each.next(), None);
    drop(each);
    assert_eq!(pair.left.node, node);
    assert_eq!(pair.text, "beta");
}

#[test]
fn wrong_language_tree_panics_with_version_skew() {
    let valid = parse(&js(), "good;");
    let value = skew::Q::find(valid.root_node(), "good;").unwrap().unwrap();
    assert_eq!(value.id.byte_range(), 0..4);

    let source = "void main() {}";
    let tree = parse(&tree_sitter_dart::LANGUAGE.into(), source);

    drop(skew::Q::each(tree.root_node(), source));
    for search in [false, true] {
        let panic = std::panic::catch_unwind(|| {
            if search {
                skew::Q::find(tree.root_node(), source)
            } else {
                skew::Q::exec(tree.root_node(), source)
            }
        })
        .expect_err("each operation must check its own tree's language");
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .expect("panic carries a message");

        assert!(message.contains("grammar version skew"), "got: {message}");
    }
}
