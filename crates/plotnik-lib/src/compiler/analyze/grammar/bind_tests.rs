use std::fmt::Write as _;

use crate::compiler::test_utils::colliding_node_kind_grammar;
use crate::compiler::{Query, QueryBuilder, SourceMap, SourcePath};

fn assert_binds_colliding_node_kinds(files: &[(&str, &str)]) {
    let grammar = colliding_node_kind_grammar();
    let mut source_map = SourceMap::new();
    for (path, content) in files {
        source_map.add_file(SourcePath::new(path), content);
    }

    let query = QueryBuilder::new(source_map).analyze().unwrap();
    if !query.is_valid() {
        panic!(
            "Expected valid query, got error:\n{}",
            query.dump_diagnostics()
        );
    }

    let query = query.bind(&grammar);
    if !query.is_valid() {
        panic!(
            "Expected valid grammar binding, got error:\n{}",
            query.dump_diagnostics()
        );
    }

    let sym = query
        .interner()
        .get("number")
        .expect("grammar-bound node name must be interned");
    let named_id = grammar.resolve_named_node("number").unwrap();
    let anonymous_id = grammar.resolve_anonymous_node("number").unwrap();

    assert_ne!(named_id, anonymous_id);
    assert_eq!(query.grammar().resolve_named_kind(sym), Some(named_id));
    assert_eq!(
        query.grammar().resolve_anonymous_kind(sym),
        Some(anonymous_id)
    );
}

#[test]
fn resolves_named_and_anonymous_node_kinds_with_same_name() {
    assert_binds_colliding_node_kinds(&[
        ("named.ptk", "A = (number)"),
        ("anonymous.ptk", "Q = \"number\""),
    ]);
    assert_binds_colliding_node_kinds(&[
        ("anonymous.ptk", "Q = \"number\""),
        ("named.ptk", "A = (number)"),
    ]);
}

#[test]
fn diamond_reference_graph_binds() {
    // Each definition references the next one twice, so the reference graph is
    // diamond-shaped. Without memoization, structural validation walks it
    // 2^depth times — intractable by this depth (issue #416); with it, instant.
    let depth = 30;
    let mut input = String::new();
    for i in 0..depth {
        writeln!(
            input,
            "D{i} = (statement_block (D{next}) (D{next}))",
            next = i + 1
        )
        .unwrap();
    }
    // Leaf is uncaptured: the two sibling references inline it transparently, so a
    // capturing leaf would (correctly) collide as a duplicate capture in scope. It is
    // a `statement_block` so every level nests a valid block child and the whole chain
    // stays satisfiable — a bare `(identifier)` is no statement and would be rejected.
    writeln!(input, "D{depth} = (statement_block)").unwrap();

    Query::expect_valid_binding(&input);
}

#[test]
fn deep_violation_in_alternation_does_not_reject_query() {
    // Skipping applies at every level nested under the alternation, not just the first.
    Query::expect_valid_binding(
        r"Q = [(call_expression function: (member_expression object: (number))) (identifier)]",
    );
}
