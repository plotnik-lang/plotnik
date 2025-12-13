//! Tests for graph construction integrated with Query pipeline.

use indoc::indoc;

use crate::query::Query;

fn snapshot(input: &str) -> String {
    let query = Query::try_from(input).unwrap().build_graph();
    query.graph().dump()
}

fn snapshot_optimized(input: &str) -> String {
    let query = Query::try_from(input).unwrap().build_graph();
    query.graph().dump_live(query.dead_nodes())
}

#[test]
fn simple_named_node() {
    insta::assert_snapshot!(snapshot("Q = (identifier)"), @r"
    Q = N0

    N0: (identifier) → ∅
    ");
}

#[test]
fn named_node_with_capture() {
    insta::assert_snapshot!(snapshot("Q = (identifier) @id"), @r"
    Q = N0

    N0: (identifier) [Capture] → N1
    N1: ε [Field(id)] → ∅
    ");
}

#[test]
fn named_node_with_children() {
    insta::assert_snapshot!(snapshot("Q = (function_definition (identifier))"), @r"
    Q = N0

    N0: (function_definition) → N1
    N1: [Down] (identifier) → N2
    N2: [Up(1)] ε → ∅
    ");
}

#[test]
fn sequence() {
    insta::assert_snapshot!(snapshot("Q = { (a) (b) }"), @r"
    Q = N1

    N0: ε [StartObj] → N1
    N1: [Next] (a) [StartObj] → N2
    N2: [Next] (b) → N3
    N3: ε [EndObj] → ∅
    ");
}

#[test]
fn sequence_with_captures() {
    insta::assert_snapshot!(snapshot("Q = { (a) @x (b) @y }"), @r"
    Q = N1

    N0: ε [StartObj] → N1
    N1: [Next] (a) [StartObj] [Capture] → N3
    N2: ε [Field(x)] → N3
    N3: [Next] (b) [Field(x)] [Capture] → N5
    N4: ε [Field(y)] → N5
    N5: ε [Field(y)] [EndObj] → ∅
    ");
}

#[test]
fn alternation_untagged() {
    insta::assert_snapshot!(snapshot("Q = [ (a) (b) ]"), @r"
    Q = N0

    N0: ε → N2, N3
    N1: ε → ∅
    N2: (a) → N1
    N3: (b) → N1
    ");
}

#[test]
fn alternation_tagged() {
    insta::assert_snapshot!(snapshot("Q = [ A: (a) @x  B: (b) @y ]"), @r"
    Q = N0

    N0: ε → N3, N7
    N1: ε [Field(x)] [EndVariant] [Field(y)] [EndVariant] → ∅
    N2: ε [Variant(A)] → N3
    N3: (a) [Variant(A)] [Capture] → N1
    N4: ε [Field(x)] → N1
    N5: ε [EndVariant] → N1
    N6: ε [Variant(B)] → N7
    N7: (b) [Variant(B)] [Capture] → N1
    N8: ε [Field(y)] → N1
    N9: ε [EndVariant] → N1
    ");
}

#[test]
fn quantifier_star() {
    insta::assert_snapshot!(snapshot("Q = (identifier)*"), @r"
    Q = N2

    N0: (identifier) → N2
    N1: ε [StartArray] → N2
    N2: ε [StartArray] [Push] → N0, N4
    N3: ε [Push] → N2
    N4: ε [EndArray] → ∅
    ");
}

#[test]
fn quantifier_plus() {
    insta::assert_snapshot!(snapshot("Q = (identifier)+"), @r"
    Q = N0

    N0: (identifier) [StartArray] → N3
    N1: ε [StartArray] → N0
    N2: ε [Push] → N3
    N3: ε [Push] → N0, N4
    N4: ε [EndArray] → ∅
    ");
}

#[test]
fn quantifier_optional() {
    insta::assert_snapshot!(snapshot("Q = (identifier)?"), @r"
    Q = N1

    N0: (identifier) → N2
    N1: ε → N0, N2
    N2: ε → ∅
    ");
}

#[test]
fn reference() {
    let input = indoc! {r#"
        A = (identifier)
        B = (A)
    "#};
    insta::assert_snapshot!(snapshot(input), @r"
    A = N0
    B = N1

    N0: (identifier) → ∅
    N1: ε +Enter(0, A) → N0
    N2: ε +Exit(0) → ∅
    ");
}

#[test]
fn anonymous_node() {
    insta::assert_snapshot!(snapshot(r#"Q = "hello""#), @r#"
    Q = N0

    N0: "hello" → ∅
    "#);
}

#[test]
fn wildcard() {
    insta::assert_snapshot!(snapshot("Q = (_)"), @r"
    Q = N0

    N0: _ → ∅
    ");
}

#[test]
fn field_constraint() {
    insta::assert_snapshot!(snapshot("Q = (function name: (identifier))"), @r"
    Q = N0

    N0: (function) → N1
    N1: [Down] (identifier) @name → N2
    N2: [Up(1)] ε → ∅
    ");
}

#[test]
fn to_string_annotation() {
    insta::assert_snapshot!(snapshot("Q = (identifier) @name ::string"), @r"
    Q = N0

    N0: (identifier) [Capture] [ToString] → N1
    N1: ε [Field(name)] → ∅
    ");
}

#[test]
fn anchor_first_child() {
    insta::assert_snapshot!(snapshot("Q = (parent . (child))"), @r"
    Q = N0

    N0: (parent) → N1
    N1: [Down.] (child) → N2
    N2: [Up(1)] ε → ∅
    ");
}

#[test]
fn anchor_sibling() {
    insta::assert_snapshot!(snapshot("Q = (parent (a) . (b))"), @r"
    Q = N0

    N0: (parent) → N1
    N1: [Down] (a) → N2
    N2: [Next.] (b) → N3
    N3: [Up(1)] ε → ∅
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Optimization tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn optimized_simple() {
    insta::assert_snapshot!(snapshot_optimized("Q = (identifier) @id"), @r"
    Q = N0

    N0: (identifier) [Capture] → N1
    N1: ε [Field(id)] → ∅
    ");
}

#[test]
fn optimized_sequence() {
    insta::assert_snapshot!(snapshot_optimized("Q = { (a) @x (b) @y }"), @r"
    Q = N1

    N1: [Next] (a) [StartObj] [Capture] → N3
    N3: [Next] (b) [Field(x)] [Capture] → N5
    N5: ε [Field(y)] [EndObj] → ∅
    ");
}

#[test]
fn symbol_table_reuse() {
    let input = indoc! {r#"
        Foo = (identifier)
        Bar = (Foo)
        Baz = (Bar)
    "#};
    let query = Query::try_from(input).unwrap().build_graph();

    assert!(query.graph().definition("Foo").is_some());
    assert!(query.graph().definition("Bar").is_some());
    assert!(query.graph().definition("Baz").is_some());

    insta::assert_snapshot!(query.graph().dump(), @r"
    Foo = N0
    Bar = N1
    Baz = N3

    N0: (identifier) → ∅
    N1: ε +Enter(0, Foo) → N0
    N2: ε +Exit(0) → ∅
    N3: ε +Enter(1, Bar) → N1
    N4: ε +Exit(1) → ∅
    ");
}
