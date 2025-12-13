//! Tests for BuildGraph construction and fragment combinators.

use super::*;

#[test]
fn single_matcher() {
    let mut g = BuildGraph::new();

    let frag = g.matcher_fragment(BuildMatcher::node("identifier"));

    assert_eq!(frag.entry, frag.exit);
    insta::assert_snapshot!(g.dump(), @r"
    N0: (identifier) → ∅
    ");
}

#[test]
fn epsilon_fragment() {
    let mut g = BuildGraph::new();

    let frag = g.epsilon_fragment();

    assert_eq!(frag.entry, frag.exit);
    insta::assert_snapshot!(g.dump(), @r"
    N0: ε → ∅
    ");
}

#[test]
fn sequence_empty() {
    let mut g = BuildGraph::new();

    let frag = g.sequence(&[]);

    assert_eq!(frag.entry, frag.exit);
    insta::assert_snapshot!(g.dump(), @r"
    N0: ε → ∅
    ");
}

#[test]
fn sequence_single() {
    let mut g = BuildGraph::new();
    let f1 = g.matcher_fragment(BuildMatcher::node("identifier"));

    let frag = g.sequence(&[f1]);

    assert_eq!(frag, f1);
    insta::assert_snapshot!(g.dump(), @r"
    N0: (identifier) → ∅
    ");
}

#[test]
fn sequence_two() {
    let mut g = BuildGraph::new();
    let f1 = g.matcher_fragment(BuildMatcher::node("identifier"));
    let f2 = g.matcher_fragment(BuildMatcher::node("number"));

    let frag = g.sequence(&[f1, f2]);

    assert_eq!(frag.entry, f1.entry);
    assert_eq!(frag.exit, f2.exit);
    insta::assert_snapshot!(g.dump(), @r"
    N0: (identifier) → N1
    N1: (number) → ∅
    ");
}

#[test]
fn sequence_three() {
    let mut g = BuildGraph::new();
    let f1 = g.matcher_fragment(BuildMatcher::node("a"));
    let f2 = g.matcher_fragment(BuildMatcher::node("b"));
    let f3 = g.matcher_fragment(BuildMatcher::node("c"));

    let frag = g.sequence(&[f1, f2, f3]);

    assert_eq!(frag.entry, f1.entry);
    assert_eq!(frag.exit, f3.exit);
    insta::assert_snapshot!(g.dump(), @r"
    N0: (a) → N1
    N1: (b) → N2
    N2: (c) → ∅
    ");
}

#[test]
fn alternation_empty() {
    let mut g = BuildGraph::new();

    let frag = g.alternation(&[]);

    assert_eq!(frag.entry, frag.exit);
    insta::assert_snapshot!(g.dump(), @r"
    N0: ε → ∅
    ");
}

#[test]
fn alternation_single() {
    let mut g = BuildGraph::new();
    let f1 = g.matcher_fragment(BuildMatcher::node("identifier"));

    let frag = g.alternation(&[f1]);

    assert_eq!(frag, f1);
    insta::assert_snapshot!(g.dump(), @r"
    N0: (identifier) → ∅
    ");
}

#[test]
fn alternation_two() {
    let mut g = BuildGraph::new();
    let f1 = g.matcher_fragment(BuildMatcher::node("identifier"));
    let f2 = g.matcher_fragment(BuildMatcher::node("number"));

    let frag = g.alternation(&[f1, f2]);

    // Entry connects to both branches, both branches connect to exit
    insta::assert_snapshot!(g.dump(), @r"
    N0: (identifier) → N3
    N1: (number) → N3
    N2: ε → N0, N1
    N3: ε → ∅
    ");
    assert_eq!(frag.entry, 2);
    assert_eq!(frag.exit, 3);
}

#[test]
fn zero_or_more_greedy() {
    let mut g = BuildGraph::new();
    let inner = g.matcher_fragment(BuildMatcher::node("item"));

    let frag = g.zero_or_more(inner);

    // Greedy: branch tries inner first, then exit
    insta::assert_snapshot!(g.dump(), @r"
    N0: (item) → N1
    N1: ε → N0, N2
    N2: ε → ∅
    ");
    assert_eq!(frag.entry, 1); // branch node
    assert_eq!(frag.exit, 2);
}

#[test]
fn zero_or_more_lazy() {
    let mut g = BuildGraph::new();
    let inner = g.matcher_fragment(BuildMatcher::node("item"));

    let frag = g.zero_or_more_lazy(inner);

    // Non-greedy: branch tries exit first, then inner
    insta::assert_snapshot!(g.dump(), @r"
    N0: (item) → N1
    N1: ε → N2, N0
    N2: ε → ∅
    ");
    assert_eq!(frag.entry, 1);
    assert_eq!(frag.exit, 2);
}

#[test]
fn one_or_more_greedy() {
    let mut g = BuildGraph::new();
    let inner = g.matcher_fragment(BuildMatcher::node("item"));

    let frag = g.one_or_more(inner);

    // Entry is inner, greedy branch after
    insta::assert_snapshot!(g.dump(), @r"
    N0: (item) → N1
    N1: ε → N0, N2
    N2: ε → ∅
    ");
    assert_eq!(frag.entry, 0); // inner node
    assert_eq!(frag.exit, 2);
}

#[test]
fn one_or_more_lazy() {
    let mut g = BuildGraph::new();
    let inner = g.matcher_fragment(BuildMatcher::node("item"));

    let frag = g.one_or_more_lazy(inner);

    // Entry is inner, non-greedy branch after
    insta::assert_snapshot!(g.dump(), @r"
    N0: (item) → N1
    N1: ε → N2, N0
    N2: ε → ∅
    ");
    assert_eq!(frag.entry, 0);
    assert_eq!(frag.exit, 2);
}

#[test]
fn optional_greedy() {
    let mut g = BuildGraph::new();
    let inner = g.matcher_fragment(BuildMatcher::node("item"));

    let frag = g.optional(inner);

    // Greedy: branch tries inner first
    insta::assert_snapshot!(g.dump(), @r"
    N0: (item) → N2
    N1: ε → N0, N2
    N2: ε → ∅
    ");
    assert_eq!(frag.entry, 1);
    assert_eq!(frag.exit, 2);
}

#[test]
fn optional_lazy() {
    let mut g = BuildGraph::new();
    let inner = g.matcher_fragment(BuildMatcher::node("item"));

    let frag = g.optional_lazy(inner);

    // Non-greedy: branch skips first
    insta::assert_snapshot!(g.dump(), @r"
    N0: (item) → N2
    N1: ε → N2, N0
    N2: ε → ∅
    ");
    assert_eq!(frag.entry, 1);
    assert_eq!(frag.exit, 2);
}

#[test]
fn matcher_with_field() {
    let mut g = BuildGraph::new();

    g.matcher_fragment(BuildMatcher::node("identifier").with_field("name"));

    insta::assert_snapshot!(g.dump(), @r"
    N0: (identifier) @name → ∅
    ");
}

#[test]
fn matcher_with_negated_fields() {
    let mut g = BuildGraph::new();

    g.matcher_fragment(
        BuildMatcher::node("call")
            .with_negated_field("arguments")
            .with_negated_field("type_arguments"),
    );

    insta::assert_snapshot!(g.dump(), @r"
    N0: (call) !arguments !type_arguments → ∅
    ");
}

#[test]
fn anonymous_matcher() {
    let mut g = BuildGraph::new();

    g.matcher_fragment(BuildMatcher::anonymous("+"));

    insta::assert_snapshot!(g.dump(), @r#"
    N0: "+" → ∅
    "#);
}

#[test]
fn wildcard_matcher() {
    let mut g = BuildGraph::new();

    g.matcher_fragment(BuildMatcher::wildcard());

    insta::assert_snapshot!(g.dump(), @r"
    N0: _ → ∅
    ");
}

#[test]
fn node_with_effects() {
    let mut g = BuildGraph::new();
    let id = g.add_matcher(BuildMatcher::node("identifier"));
    g.node_mut(id).add_effect(BuildEffect::CaptureNode);
    g.node_mut(id).add_effect(BuildEffect::Field("name"));

    insta::assert_snapshot!(g.dump(), @r"
    N0: (identifier) [Capture] [Field(name)] → ∅
    ");
}

#[test]
fn node_with_ref_marker() {
    let mut g = BuildGraph::new();
    let enter = g.add_epsilon();
    g.node_mut(enter).set_ref_marker(RefMarker::enter(0));

    let exit = g.add_epsilon();
    g.node_mut(exit).set_ref_marker(RefMarker::exit(0));

    g.connect(enter, exit);

    insta::assert_snapshot!(g.dump(), @r"
    N0: ε +Enter(0, ?) → N1
    N1: ε +Exit(0) → ∅
    ");
}

#[test]
fn definition_registration() {
    let mut g = BuildGraph::new();
    let f1 = g.matcher_fragment(BuildMatcher::node("identifier"));
    g.add_definition("Ident", f1.entry);

    let f2 = g.matcher_fragment(BuildMatcher::node("number"));
    g.add_definition("Num", f2.entry);

    assert_eq!(g.definition("Ident"), Some(0));
    assert_eq!(g.definition("Num"), Some(1));
    assert_eq!(g.definition("Unknown"), None);

    insta::assert_snapshot!(g.dump(), @r"
    Ident = N0
    Num = N1

    N0: (identifier) → ∅
    N1: (number) → ∅
    ");
}

#[test]
fn complex_nested_structure() {
    let mut g = BuildGraph::new();

    // Build: (func { (identifier)+ (block) })
    let ident = g.matcher_fragment(BuildMatcher::node("identifier"));
    let idents = g.one_or_more(ident);

    let block = g.matcher_fragment(BuildMatcher::node("block"));
    let body = g.sequence(&[idents, block]);

    let func = g.matcher_fragment(BuildMatcher::node("func"));
    g.connect_exit(func, body.entry);

    g.add_definition("Func", func.entry);

    insta::assert_snapshot!(g.dump(), @r"
    Func = N4

    N0: (identifier) → N1
    N1: ε → N0, N2
    N2: ε → N3
    N3: (block) → ∅
    N4: (func) → N0
    ");
}
