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

fn native_json<'t, 's, E: plotnik::rt::Publish<'t, 's>>(
    value: &plotnik::rt::Match<E>,
    source: &str,
) -> serde_json::Value {
    let mut output = String::new();
    value
        .write_json(source, plotnik::rt::json::Style::COMPACT, &mut output)
        .expect("native result renders");
    serde_json::from_str(&output).expect("native result emits valid JSON")
}

fn document_json(value: &plotnik_compiler::Document<'_, '_, '_>) -> serde_json::Value {
    let mut output = String::new();
    value
        .write_json(plotnik::rt::json::Style::COMPACT, &mut output)
        .expect("document renders");
    serde_json::from_str(&output).expect("document emits valid JSON")
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

mod limited {
    plotnik::query! {
        "Q = (program (expression_statement (identifier) @id))",
        grammar = "tree-sitter-javascript",
        fuel = 1,
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

mod walk_limited {
    plotnik::query! {
        "Probe = (expression_statement) Root = (_)",
        grammar = "tree-sitter-javascript",
        fuel = 25,
    }
}

mod application_limited {
    plotnik::query! {
        "Probe = (_)",
        grammar = "tree-sitter-javascript",
        fuel = 3,
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
        .expect("auto limits fit")
        .expect("matches");
    let json = native_json(&value, source);

    assert_eq!(value.view().id.utf8_text(source.as_bytes()), Ok("x"));
    let found = plotnik::find::<queries::Idents>(tree.root_node(), source)
        .unwrap()
        .unwrap();
    assert_eq!(native_json(&found, source), json);
    let all = plotnik::find_all::<queries::Idents>(tree.root_node(), source).unwrap();
    assert_eq!(
        (&all).into_iter().map(|q| q.id).collect::<Vec<_>>(),
        vec![value.view().id]
    );
    let each = plotnik::each::<queries::Idents>(tree.root_node(), source)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        each.iter().map(|q| q.view().id).collect::<Vec<_>>(),
        vec![value.view().id]
    );
    assert_eq!(
        plotnik::exec::<queries::Probe>(tree.root_node(), source),
        Ok(Some(tree.root_node()))
    );
    assert_eq!(
        json,
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
            .view()
            .id
            .utf8_text(source.as_bytes()),
        Ok("x")
    );
    assert_eq!(
        tree_sitter_package::Q::exec(javascript_tree.root_node(), source)
            .unwrap()
            .unwrap()
            .view()
            .id
            .utf8_text(source.as_bytes()),
        Ok("x")
    );
    assert_eq!(
        tsx::Q::exec(tsx_tree.root_node(), source)
            .unwrap()
            .unwrap()
            .view()
            .id
            .utf8_text(source.as_bytes()),
        Ok("x")
    );
}

#[test]
fn compiled_limits_reject_work_at_their_distinct_boundaries() {
    let source = "((x));";
    let tree = parse(&js(), source);

    assert!(matches!(
        limited::Q::exec(tree.root_node(), source),
        Err(LimitExceeded::OutOfFuel(1))
    ));
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
        nodes::Id::exec(alpha, source).unwrap().unwrap().view().text,
        "alpha"
    );
    assert_eq!(nodes::Probe::exec(alpha, source), Ok(Some(alpha)));
    assert!(nodes::Id::exec(binary, source).unwrap().is_none());
    assert!(nodes::Id::exec(tree.root_node(), source).unwrap().is_none());
    assert_eq!(
        nodes::Probe::find_all(binary, source)
            .unwrap()
            .iter()
            .map(|node| node.utf8_text(source.as_bytes()).unwrap())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta"]
    );
    let pair = nodes::Pair::exec(binary, source).unwrap().unwrap();
    assert_eq!(pair.view().left.node, alpha);
    assert_eq!(pair.view().text, "beta");
    let token = nodes::Token::find(binary, source).unwrap().unwrap();
    assert!(!token.is_named());
    assert_eq!(token.utf8_text(source.as_bytes()), Ok("+"));
    assert!(nodes::Token::find_all(alpha, source).unwrap().is_empty());
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
        .map(|value| match value.view() {
            nodes::Binary::First { left } => left.utf8_text(source.as_bytes()).unwrap(),
            nodes::Binary::Second { .. } => panic!("source-order preference selects First"),
        })
        .collect::<Vec<_>>();

    assert_eq!(lefts, vec!["alpha", "delta"]);
    assert_eq!(
        native_json(
            &nodes::Binary::find(scope, source).unwrap().unwrap(),
            source
        ),
        native_json(&values[0], source)
    );
    let all = nodes::Binary::find_all(scope, source).unwrap();
    let all_lefts: Vec<_> = (&all)
        .into_iter()
        .map(|value| match value {
            nodes::Binary::First { left } => left.utf8_text(source.as_bytes()).unwrap(),
            nodes::Binary::Second { .. } => panic!("source-order preference selects First"),
        })
        .collect();
    assert_eq!(all_lefts, lefts);
    let outer = scope.named_child(0).unwrap().named_child(0).unwrap();
    assert_eq!(
        native_json(
            &nodes::Binary::exec(outer, source).unwrap().unwrap(),
            source
        ),
        native_json(&values[0], source)
    );
    assert_eq!(
        (&nodes::Binary::find_all(outer, source).unwrap())
            .into_iter()
            .count(),
        1
    );
}

#[test]
fn quantifier_lists_stay_inside_independent_results() {
    let source = "f(alpha, beta); g(gamma);";
    let tree = parse(&js(), source);

    let mut each = nodes::Args::each(tree.root_node(), source);
    let first = each.next().unwrap().unwrap();
    let second = each.next().unwrap().unwrap();
    assert!(each.next().is_none());
    drop(each);

    assert_eq!(first.view().ids, ["alpha", "beta"]);
    assert_eq!(second.view().ids, ["gamma"]);
    let all = nodes::Args::find_all(tree.root_node(), source).unwrap();
    assert_eq!(
        (&all).into_iter().map(|q| q.ids).collect::<Vec<_>>(),
        vec![first.view().ids, second.view().ids]
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
    assert!(each.next().is_none());
    drop(each);

    let json = serde_json::json!([
        native_json(&first, source),
        native_json(&second, source),
        native_json(&third, source)
    ]);
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
            fuel_limit: Limit::Of(2500),
            ..Default::default()
        })
        .build()
        .traverse(&module, module.entrypoint("Q").unwrap());
    let first = each.next().unwrap().unwrap();
    let second = each.next().unwrap().unwrap();
    let rendered = serde_json::json!([document_json(&first), document_json(&second)]);
    assert!(matches!(
        each.next(),
        Some(Err(LimitExceeded::OutOfFuel(2500)))
    ));
    assert!(each.next().is_none());
    assert!(each.next().is_none());
    drop(each);

    assert_eq!(
        rendered,
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
        nodes::Id::find(tree.root_node(), &source)
            .unwrap()
            .unwrap()
            .view()
            .text
    };
    assert_eq!(text, "alpha");

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
    assert!(each.next().is_none());
    drop(each);
    assert_eq!(pair.view().left.node, node);
    assert_eq!(pair.view().text, "beta");
}

#[test]
fn lazy_search_shares_fuel_and_fuses_after_a_terminal_error() {
    let source = "alpha; beta; gamma; delta;";
    let tree = parse(&js(), source);
    let scope = tree.root_node();

    assert_eq!(
        application_limited::Probe::exec(scope, source),
        Ok(Some(scope))
    );
    assert_eq!(
        application_limited::Probe::each(scope, source).next(),
        Some(Err(LimitExceeded::OutOfFuel(3)))
    );
    assert_eq!(
        walk_limited::Root::find_all(scope, source)
            .unwrap()
            .as_slice(),
        &[scope]
    );

    let mut each = walk_limited::Probe::each(scope, source);
    let first = each.next().unwrap().unwrap();
    assert_eq!(first, scope.named_child(0).unwrap());
    assert_eq!(walk_limited::Probe::find(scope, source), Ok(Some(first)));
    assert_eq!(each.next(), Some(Err(LimitExceeded::OutOfFuel(25))));
    assert_eq!(each.next(), None);
    assert_eq!(each.next(), None);
    drop(each);
    assert_eq!(first.utf8_text(source.as_bytes()), Ok("alpha;"));
    assert!(matches!(
        walk_limited::Probe::find_all(scope, source),
        Err(LimitExceeded::OutOfFuel(25))
    ));
}

#[test]
fn wrong_language_tree_panics_with_version_skew() {
    let valid = parse(&js(), "good;");
    let value = skew::Q::find(valid.root_node(), "good;").unwrap().unwrap();
    assert_eq!(value.view().id.byte_range(), 0..4);

    let source = "void main() {}";
    let tree = parse(&tree_sitter_dart::LANGUAGE.into(), source);

    for operation in 0..3 {
        let panic = std::panic::catch_unwind(|| match operation {
            0 => {
                drop(skew::Q::each(tree.root_node(), source));
            }
            1 => {
                drop(skew::Q::find(tree.root_node(), source));
            }
            _ => {
                drop(skew::Q::exec(tree.root_node(), source));
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

#[test]
fn vm_inspection_rendering_allocation_failures_are_errors_and_release_storage() {
    use plotnik_compiler::{
        BytecodeConfig, BytecodeInspection, Limit, QueryBuilder, RuntimeError, RuntimeLimitSpec,
        TraceRecorder, VM, extract_result_provenance,
    };
    use plotnik_runtime::{Text, failpoints};
    let source = "f(alpha, beta);";
    let tree = parse(&js(), source);
    let module = QueryBuilder::from_inline("Q = (program (expression_statement (call_expression arguments: (arguments (identifier)* @args :: text))))")
        .compile(support::javascript_grammar()).unwrap()
        .emit(BytecodeConfig::new().inspection(BytecodeInspection::Spans)).unwrap().into_artifact().unwrap();
    let baseline = failpoints::live_bytes();
    let run = || -> Result<(), LimitExceeded> {
        let vm = VM::builder(source, &tree)
            .limits(RuntimeLimitSpec {
                fuel_limit: Limit::Unbounded,
                memory: Limit::Unbounded,
            })
            .build();
        let mut tracer = TraceRecorder::new(&module, 1000);
        let document = match vm.execute_with(&module, &module.entrypoint("Q").unwrap(), &mut tracer)
        {
            Ok(document) => document,
            Err(RuntimeError::Limit(error)) => return Err(error),
            Err(error) => panic!("fixture must match: {error}"),
        };
        let mut output = Text::new(document.account());
        document
            .write_json(plotnik_runtime::json::Style::COMPACT, &mut output)
            .map_err(|error| {
                output.error().unwrap_or_else(|| match error {
                    plotnik_runtime::json::JsonError::Limit(error) => error,
                    _ => panic!("valid source and sink"),
                })
            })?;
        let provenance = extract_result_provenance(&document, &module)?;
        assert!(!provenance.is_empty());
        assert_eq!(&*output, r#"{"args":["alpha","beta"]}"#);
        Ok(())
    };
    failpoints::reset();
    run().unwrap();
    let allocations = failpoints::allocations();

    for index in 0..allocations {
        failpoints::reset();
        failpoints::fail_at(Some(index));
        assert!(
            matches!(run(), Err(LimitExceeded::Allocation { .. })),
            "allocation {index}"
        );
        assert_eq!(
            failpoints::allocations(),
            index + 1,
            "cleanup must not allocate"
        );
        assert_eq!(failpoints::live_bytes(), baseline);
    }
    failpoints::reset();
}

#[test]
fn checked_cursor_matches_stock_navigation_and_recovers_from_interrupted_growth() {
    use plotnik_runtime::{Account, CursorWrapper, Nav, ResolvedRuntimeLimits, failpoints};
    for (language, source) in [
        (
            js(),
            "function f({x: y}) { return y + 2; } // extra\nf(alpha, beta); let z = ;".to_string(),
        ),
        (
            tree_sitter_dart::LANGUAGE.into(),
            "void main() { var x = [1, 2]; print(x); } // extra".to_string(),
        ),
    ] {
        assert!(matches!(language.abi_version(), 14 | 15));
        let tree = parse(&language, &source);
        let mut stock = tree.walk();
        let mut expected = vec![stock.node()];
        loop {
            if stock.goto_first_child() {
                expected.push(stock.node());
                continue;
            }
            loop {
                if stock.goto_next_sibling() {
                    expected.push(stock.node());
                    break;
                }
                if !stock.goto_parent() {
                    break;
                }
            }
            if stock.node() == tree.root_node() {
                break;
            }
        }
        let account = Account::new(ResolvedRuntimeLimits {
            fuel_limit: None,
            max_memory: None,
        })
        .unwrap();
        let mut meter = account.meter();
        let mut checked = CursorWrapper::new(tree.root_node());
        for (index, &node) in expected.iter().enumerate().rev() {
            checked.goto_descendant(&mut meter, index as u32).unwrap();
            assert_eq!(checked.node(), node);
            for nav in [Nav::DownExact, Nav::NextExact, Nav::Up(1)] {
                checked.goto_descendant(&mut meter, index as u32).unwrap();
                stock.goto_descendant(index);
                let moved = match nav {
                    Nav::DownExact => stock.goto_first_child(),
                    Nav::NextExact => stock.goto_next_sibling(),
                    Nav::Up(1) => stock.goto_parent(),
                    _ => unreachable!(),
                };

                assert_eq!(checked.navigate(&mut meter, nav).unwrap().is_some(), moved);
                assert_eq!(checked.node(), stock.node());
            }
        }
        checked.goto_descendant(&mut meter, 0).unwrap();
        assert!(
            checked
                .navigate(&mut meter, Nav::DownExact)
                .unwrap()
                .is_some()
        );
        assert_eq!(checked.node(), expected[1]);
        assert!(checked.navigate(&mut meter, Nav::Up(1)).unwrap().is_some());
        assert_eq!(checked.node(), tree.root_node());
    }

    let source = format!("{}x{};", "(".repeat(100), ")".repeat(100));
    let tree = parse(&js(), &source);
    let account = Account::new(ResolvedRuntimeLimits {
        fuel_limit: None,
        max_memory: None,
    })
    .unwrap();
    let mut meter = account.meter();
    let mut checked = CursorWrapper::new(tree.root_node());
    checked.navigate(&mut meter, Nav::DownExact).unwrap();
    failpoints::reset();
    failpoints::fail_at(Some(0));
    assert!(matches!(
        checked.goto_descendant(&mut meter, 100),
        Err(LimitExceeded::Allocation { .. })
    ));
    failpoints::reset();
    checked.goto_descendant(&mut meter, 0).unwrap();
    assert_eq!(checked.node(), tree.root_node());
    checked.goto_descendant(&mut meter, 100).unwrap();
    let mut stock = tree.walk();
    stock.goto_descendant(100);
    assert_eq!(checked.node(), stock.node());
}
