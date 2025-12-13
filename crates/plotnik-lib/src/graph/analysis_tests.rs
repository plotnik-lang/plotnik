//! Tests for analysis module.

use std::collections::HashSet;

use rowan::TextRange;

use super::*;
use crate::graph::{BuildEffect, BuildGraph, BuildMatcher, RefMarker};

#[test]
fn string_interner_deduplicates() {
    let mut interner = StringInterner::new();

    let id1 = interner.intern("name");
    let id2 = interner.intern("value");
    let id3 = interner.intern("name"); // duplicate

    assert_eq!(id1, id3);
    assert_ne!(id1, id2);
    assert_eq!(interner.len(), 2);
}

#[test]
fn string_interner_preserves_order() {
    let mut interner = StringInterner::new();

    interner.intern("alpha");
    interner.intern("beta");
    interner.intern("gamma");

    let strings: Vec<_> = interner.iter().collect();

    assert_eq!(strings, vec![("alpha", 0), ("beta", 1), ("gamma", 2)]);
}

#[test]
fn string_interner_total_bytes() {
    let mut interner = StringInterner::new();

    interner.intern("foo");
    interner.intern("bar");
    interner.intern("foo"); // duplicate, not counted twice

    assert_eq!(interner.total_bytes(), 6); // "foo" + "bar"
}

#[test]
fn analyze_empty_graph() {
    let g = BuildGraph::new();
    let dead = HashSet::new();

    let result = analyze(&g, &dead);

    assert_eq!(result.transition_count, 0);
    assert_eq!(result.effect_count, 0);
    assert_eq!(result.entrypoint_count, 0);
    assert!(result.strings.is_empty());
}

#[test]
fn analyze_single_matcher() {
    let mut g = BuildGraph::new();
    g.add_matcher(BuildMatcher::node("identifier"));
    let dead = HashSet::new();

    let result = analyze(&g, &dead);

    assert_eq!(result.transition_count, 1);
    assert_eq!(result.node_map[0], Some(0));
    assert_eq!(result.strings.len(), 1);
    assert_eq!(result.strings.get("identifier"), Some(0));
}

#[test]
fn analyze_skips_dead_nodes() {
    let mut g = BuildGraph::new();
    let n0 = g.add_matcher(BuildMatcher::node("a"));
    let n1 = g.add_epsilon(); // will be dead
    let n2 = g.add_matcher(BuildMatcher::node("b"));
    g.connect(n0, n1);
    g.connect(n1, n2);

    let mut dead = HashSet::new();
    dead.insert(n1);

    let result = analyze(&g, &dead);

    assert_eq!(result.transition_count, 2);
    assert_eq!(result.node_map[0], Some(0));
    assert_eq!(result.node_map[1], None); // dead
    assert_eq!(result.node_map[2], Some(1));
}

#[test]
fn analyze_counts_effects() {
    let mut g = BuildGraph::new();
    let id = g.add_matcher(BuildMatcher::node("identifier"));
    g.node_mut(id).add_effect(BuildEffect::CaptureNode);
    g.node_mut(id).add_effect(BuildEffect::Field {
        name: "name",
        span: TextRange::default(),
    });
    g.node_mut(id).add_effect(BuildEffect::ToString);

    let dead = HashSet::new();
    let result = analyze(&g, &dead);

    assert_eq!(result.effect_count, 3);
    // "identifier" and "name" interned
    assert_eq!(result.strings.len(), 2);
}

#[test]
fn analyze_counts_negated_fields() {
    let mut g = BuildGraph::new();
    g.add_matcher(
        BuildMatcher::node("call")
            .with_negated_field("arguments")
            .with_negated_field("type_arguments"),
    );

    let dead = HashSet::new();
    let result = analyze(&g, &dead);

    assert_eq!(result.negated_field_count, 2);
    // "call", "arguments", "type_arguments" interned
    assert_eq!(result.strings.len(), 3);
}

#[test]
fn analyze_interns_field_constraints() {
    let mut g = BuildGraph::new();
    g.add_matcher(BuildMatcher::node("function").with_field("name"));

    let dead = HashSet::new();
    let result = analyze(&g, &dead);

    assert_eq!(result.strings.len(), 2);
    assert!(result.strings.get("function").is_some());
    assert!(result.strings.get("name").is_some());
}

#[test]
fn analyze_interns_anonymous_literals() {
    let mut g = BuildGraph::new();
    g.add_matcher(BuildMatcher::anonymous("+"));
    g.add_matcher(BuildMatcher::anonymous("-"));
    g.add_matcher(BuildMatcher::anonymous("+")); // duplicate

    let dead = HashSet::new();
    let result = analyze(&g, &dead);

    assert_eq!(result.transition_count, 3);
    assert_eq!(result.strings.len(), 2); // "+" and "-"
}

#[test]
fn analyze_interns_variant_tags() {
    let mut g = BuildGraph::new();
    let n0 = g.add_epsilon();
    g.node_mut(n0).add_effect(BuildEffect::StartVariant("True"));

    let n1 = g.add_epsilon();
    g.node_mut(n1)
        .add_effect(BuildEffect::StartVariant("False"));

    let dead = HashSet::new();
    let result = analyze(&g, &dead);

    assert_eq!(result.strings.len(), 2);
    assert!(result.strings.get("True").is_some());
    assert!(result.strings.get("False").is_some());
}

#[test]
fn analyze_counts_entrypoints() {
    let mut g = BuildGraph::new();
    let f1 = g.matcher_fragment(BuildMatcher::node("identifier"));
    g.add_definition("Ident", f1.entry);

    let f2 = g.matcher_fragment(BuildMatcher::node("number"));
    g.add_definition("Num", f2.entry);

    let dead = HashSet::new();
    let result = analyze(&g, &dead);

    assert_eq!(result.entrypoint_count, 2);
    // "identifier", "number", "Ident", "Num" interned
    assert_eq!(result.strings.len(), 4);
}

#[test]
fn analyze_deduplicates_across_sources() {
    let mut g = BuildGraph::new();

    // "name" appears as: node kind, field constraint, effect field, definition name
    let n0 = g.add_matcher(BuildMatcher::node("name").with_field("name"));
    g.node_mut(n0).add_effect(BuildEffect::Field {
        name: "name",
        span: TextRange::default(),
    });
    g.add_definition("name", n0);

    let dead = HashSet::new();
    let result = analyze(&g, &dead);

    // All "name" references should resolve to same StringId
    assert_eq!(result.strings.len(), 1);
    assert_eq!(result.strings.get("name"), Some(0));
}

#[test]
fn analyze_wildcard_with_field() {
    let mut g = BuildGraph::new();
    g.add_matcher(BuildMatcher::wildcard().with_field("body"));

    let dead = HashSet::new();
    let result = analyze(&g, &dead);

    assert_eq!(result.strings.len(), 1);
    assert!(result.strings.get("body").is_some());
}

#[test]
fn analyze_ref_names() {
    let mut g = BuildGraph::new();
    let enter = g.add_epsilon();
    g.node_mut(enter).set_ref_marker(RefMarker::enter(0));
    g.node_mut(enter).ref_name = Some("Function");

    let dead = HashSet::new();
    let result = analyze(&g, &dead);

    assert_eq!(result.strings.len(), 1);
    assert!(result.strings.get("Function").is_some());
}

#[test]
fn node_map_indices_are_contiguous() {
    let mut g = BuildGraph::new();
    g.add_matcher(BuildMatcher::node("a")); // 0 -> 0
    g.add_epsilon(); // 1 -> dead
    g.add_matcher(BuildMatcher::node("b")); // 2 -> 1
    g.add_epsilon(); // 3 -> dead
    g.add_matcher(BuildMatcher::node("c")); // 4 -> 2

    let mut dead = HashSet::new();
    dead.insert(1);
    dead.insert(3);

    let result = analyze(&g, &dead);

    assert_eq!(result.transition_count, 3);
    assert_eq!(result.node_map[0], Some(0));
    assert_eq!(result.node_map[1], None);
    assert_eq!(result.node_map[2], Some(1));
    assert_eq!(result.node_map[3], None);
    assert_eq!(result.node_map[4], Some(2));
}
