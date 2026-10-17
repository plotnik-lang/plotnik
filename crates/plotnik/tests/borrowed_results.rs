#![forbid(unsafe_code)]

use plotnik::rt::{LimitExceeded, failpoints, json::Style};
use plotnik::tree_sitter::{Parser, Tree};

mod list {
    plotnik::query! {
        "Q = (program (expression_statement (identifier) @id)* @items)",
        grammar = "tree-sitter-javascript",
        fuel = unbounded,
        memory = unbounded,
    }
}

mod leaf {
    plotnik::query! {
        "Q = (identifier) @id",
        grammar = "tree-sitter-javascript",
        fuel = unbounded,
        memory = unbounded,
    }
}

mod recursive {
    plotnik::query! {
        "Expr = [Paren: (parenthesized_expression (Expr) @inner) Id: (identifier) @id] Q = (program (expression_statement (Expr) @expr))",
        grammar = "tree-sitter-javascript",
        fuel = unbounded,
        memory = unbounded,
    }
}

mod mixed {
    plotnik::query! {
        "Q = (program (expression_statement (identifier) @node) (expression_statement (identifier) @text :: text)* @texts)",
        grammar = "tree-sitter-javascript",
        fuel = unbounded,
        memory = unbounded,
    }
}

mod text {
    plotnik::query! {
        "Q = (identifier) @text :: text",
        grammar = "tree-sitter-javascript",
        fuel = unbounded,
        memory = unbounded,
    }
}

fn parse(source: &str) -> Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .expect("javascript language loads");
    parser.parse(source, None).expect("source parses")
}

#[test]
fn list_values_and_json_preserve_source_order() {
    let source = "alpha; beta;";
    let tree = parse(source);

    let result = list::Q::exec(tree.root_node(), source).unwrap().unwrap();
    let mut json = String::new();
    result
        .write_json(source, Style::COMPACT, &mut json)
        .unwrap();

    assert_eq!(result.view().items.len(), 2);
    assert_eq!(result.view().items[0].id.start_byte(), 0);
    assert_eq!(result.view().items[1].id.start_byte(), 7);
    assert_eq!(
        json,
        r#"{"items":[{"id":{"kind":"identifier","text":"alpha","span":[0, 5]}},{"id":{"kind":"identifier","text":"beta","span":[7, 11]}}]}"#
    );
}

#[test]
fn each_items_outlive_iterator_and_copied_leaves_outlive_owners() {
    let source = "alpha; beta;";
    let tree = parse(source);
    let mut each = leaf::Q::each(tree.root_node(), source);

    let first = each.next().unwrap().unwrap();
    let second = each.next().unwrap().unwrap();
    drop(each);
    let node = first.view().id;
    drop(first);

    assert_eq!(node.utf8_text(source.as_bytes()).unwrap(), "alpha");
    assert_eq!(
        second.view().id.utf8_text(source.as_bytes()).unwrap(),
        "beta"
    );
}

#[test]
fn owners_declared_before_tree_allow_normal_reverse_drop_order() {
    let one;
    let all;
    let source = "a;b;c;d;e;f;g;h;i;j;k;l;m;n;o;p;q;";
    let tree = parse(source);

    one = leaf::Q::find(tree.root_node(), source).unwrap().unwrap();
    all = leaf::Q::find_all(tree.root_node(), source).unwrap();

    assert_eq!(one.view().id.start_byte(), 0);
    let starts: Vec<_> = (&all).into_iter().map(|q| q.id.start_byte()).collect();
    assert_eq!(starts, (0..=32).step_by(2).collect::<Vec<_>>());
    assert_eq!((&all).into_iter().next_back().unwrap().id.start_byte(), 32);
}

#[test]
fn tree_source_and_result_storage_have_distinct_lifetimes() {
    let result;
    let source = String::from("a;b;c;");
    let tree = parse(&source);

    result = mixed::Q::exec(tree.root_node(), &source).unwrap().unwrap();
    let text_only = {
        let tree = parse(&source);
        text::Q::find(tree.root_node(), &source).unwrap().unwrap()
    };

    assert_eq!(
        result.view().node.utf8_text(source.as_bytes()).unwrap(),
        "a"
    );
    assert_eq!(
        result
            .view()
            .texts
            .iter()
            .map(|item| item.text)
            .collect::<Vec<_>>(),
        ["b", "c"]
    );
    assert_eq!(text_only.view().text, "a");
}

#[test]
fn deep_construction_json_and_drop_fit_small_stack() {
    let depth = 3000;
    let source = format!("{}x{};", "(".repeat(depth), ")".repeat(depth));
    let tree = parse(&source);

    std::thread::scope(|threads| {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn_scoped(threads, || {
                let result = recursive::Q::exec(tree.root_node(), &source).unwrap().unwrap();
                let mut expr = &result.view().expr;
                let mut actual = 0;
                while let recursive::Expr::Paren { inner } = expr {
                    actual += 1;
                    expr = inner;
                }
                assert_eq!(actual, depth);

                let mut json = String::new();
                result.write_json(&source, Style::COMPACT, &mut json).unwrap();
                let expected = format!(
                    "{{\"expr\":{}{{\"$tag\":\"Id\",\"$data\":{{\"id\":{{\"kind\":\"identifier\",\"text\":\"x\",\"span\":[3000, 3001]}}}}}}{}}}",
                    r#"{"$tag":"Paren","$data":{"inner":"#.repeat(depth),
                    "}}".repeat(depth),
                );
                assert_eq!(json, expected);
                drop(result);
            })
            .unwrap()
            .join()
            .unwrap();
    });
}

#[test]
fn publication_allocation_failures_release_every_admitted_block() {
    let source = "a;b;c;d;e;f;g;h;i;j;k;l;m;n;o;p;q;";
    let tree = parse(source);
    let baseline = failpoints::live_bytes();
    let operations: [&dyn Fn() -> Result<(), LimitExceeded>; 2] = [
        &|| leaf::Q::find_all(tree.root_node(), source).map(drop),
        &|| list::Q::find_all(tree.root_node(), source).map(drop),
    ];

    for run in operations {
        failpoints::reset();
        run().unwrap();
        let allocations = failpoints::allocations();
        assert!(allocations > 0);
        assert_eq!(failpoints::live_bytes(), baseline);

        for index in 0..allocations {
            failpoints::reset();
            failpoints::fail_at(Some(index));

            let result = run();

            assert!(matches!(result, Err(LimitExceeded::Allocation { .. })));
            assert_eq!(failpoints::allocations(), index + 1);
            assert_eq!(failpoints::live_bytes(), baseline);
        }
    }
    failpoints::reset();
}
