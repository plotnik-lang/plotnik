use crate::compiler::test_utils::colliding_node_kind_grammar;
use crate::compiler::{Query, QueryBuilder, SourceMap, SourcePath};

fn assert_links_colliding_node_kinds(files: &[(&str, &str)]) {
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
    assert_eq!(query.grammar().resolve_named_kind(sym), Some(named_id));
    assert_eq!(
        query.grammar().resolve_anonymous_kind(sym),
        Some(anonymous_id)
    );
}

#[test]
fn resolves_named_and_anonymous_node_kinds_with_same_name() {
    assert_links_colliding_node_kinds(&[
        ("named.ptk", "A = (number)"),
        ("anonymous.ptk", "Q = \"number\""),
    ]);
    assert_links_colliding_node_kinds(&[
        ("anonymous.ptk", "Q = \"number\""),
        ("named.ptk", "A = (number)"),
    ]);
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
    // Leaf is uncaptured: the two sibling references inline it transparently, so a
    // capturing leaf would (correctly) collide as a duplicate capture in scope. The
    // diamond shape — and the memoization it stresses — is unchanged.
    input.push_str(&format!("D{depth} = (identifier)\n"));

    Query::expect_valid_linking(&input);
}

#[test]
fn deep_violation_in_alternation_does_not_reject_query() {
    // Skipping applies at every level nested under the alternation, not just the first.
    Query::expect_valid_linking(
        r"Q = [(call_expression function: (member_expression object: (number))) (identifier)]",
    );
}
