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
    Q = (0)

    (0) —(identifier)→ (✓)
    ");
}

#[test]
fn named_node_with_capture() {
    insta::assert_snapshot!(snapshot("Q = (identifier) @id"), @r"
    Q = (0)

    (0) —(identifier)—[CaptureNode]→ (✓)
    ");
}

#[test]
fn named_node_with_children() {
    insta::assert_snapshot!(snapshot("Q = (function_definition (identifier))"), @r"
    Q = (0)

    (0) —(function_definition)→ (1)
    (1) —{↘}—(identifier)→ (2)
    (2) —{↗¹}—𝜀→ (✓)
    ");
}

#[test]
fn sequence() {
    insta::assert_snapshot!(snapshot("Q = { (a) (b) }"), @r"
    Q = (1)

    (0) —𝜀→ (1)
    (1) —{→}—(a)→ (2)
    (2) —{→}—(b)→ (✓)
    ");
}

#[test]
fn sequence_with_captures() {
    insta::assert_snapshot!(snapshot("Q = { (a) @x (b) @y }"), @r"
    Q = (0)

    (0) —𝜀—[StartObject]→ (1)
    (1) —{→}—(a)—[CaptureNode]→ (2)
    (2) —𝜀—[Field(x)]→ (3)
    (3) —{→}—(b)—[CaptureNode]→ (6)
    (4) —𝜀—[Field(y)]→ (6)
    (5) —𝜀—[StartObject]→ (0)
    (6) —𝜀—[Field(y), EndObject]→ (✓)
    ");
}

#[test]
fn alternation_untagged() {
    insta::assert_snapshot!(snapshot("Q = [ (a) (b) ]"), @r"
    Q = (0)

    (0) —𝜀→ (2), (3)
    (1) —𝜀→ (✓)
    (2) —(a)→ (1)
    (3) —(b)→ (1)
    ");
}

#[test]
fn alternation_tagged() {
    insta::assert_snapshot!(snapshot("Q = [ A: (a) @x  B: (b) @y ]"), @r"
    Q = (00)

    (00) —𝜀—[StartObject]→ (03), (07)
    (01) —𝜀→ (11)
    (02) —𝜀—[StartVariant(A)]→ (03)
    (03) —(a)—[StartVariant(A), CaptureNode]→ (05)
    (04) —𝜀—[Field(x)]→ (05)
    (05) —𝜀—[Field(x), EndVariant]→ (11)
    (06) —𝜀—[StartVariant(B)]→ (07)
    (07) —(b)—[StartVariant(B), CaptureNode]→ (09)
    (08) —𝜀—[Field(y)]→ (09)
    (09) —𝜀—[Field(y), EndVariant]→ (11)
    (10) —𝜀—[StartObject]→ (00)
    (11) —𝜀—[EndObject]→ (✓)
    ");
}

#[test]
fn quantifier_star() {
    insta::assert_snapshot!(snapshot("Q = (identifier)*"), @r"
    Q = (1)

    (0) —(identifier)→ (3)
    (1) —𝜀—[StartArray]→ (4)
    (2) —𝜀—[EndArray]→ (✓)
    (3) —𝜀—[PushElement]→ (4)
    (4) —𝜀→ (0), (2)
    ");
}

#[test]
fn quantifier_plus() {
    insta::assert_snapshot!(snapshot("Q = (identifier)+"), @r"
    Q = (1)

    (0) —(identifier)→ (4)
    (1) —𝜀—[StartArray]→ (0)
    (2) —𝜀—[EndArray]→ (✓)
    (3) —𝜀—[PushElement]→ (4)
    (4) —𝜀—[PushElement]→ (0), (2)
    ");
}

#[test]
fn quantifier_optional() {
    insta::assert_snapshot!(snapshot("Q = (identifier)?"), @r"
    Q = (1)

    (0) —(identifier)→ (2)
    (1) —𝜀→ (0), (3)
    (2) —𝜀→ (✓)
    (3) —𝜀—[ClearCurrent]→ (2)
    ");
}

#[test]
fn reference() {
    let input = indoc! {r#"
        A = (identifier)
        B = (A)
    "#};
    insta::assert_snapshot!(snapshot(input), @r"
    A = (0)
    B = (1)

    (0) —(identifier)→ (✓)
    (1) —<A>—𝜀→ (0), (2)
    (2) —𝜀—<A>→ (✓)
    ");
}

#[test]
fn anonymous_node() {
    insta::assert_snapshot!(snapshot(r#"Q = "hello""#), @r#"
    Q = (0)

    (0) —"hello"→ (✓)
    "#);
}

#[test]
fn wildcard() {
    insta::assert_snapshot!(snapshot("Q = (_)"), @r"
    Q = (0)

    (0) —(🞵)→ (✓)
    ");
}

#[test]
fn field_constraint() {
    insta::assert_snapshot!(snapshot("Q = (function name: (identifier))"), @r"
    Q = (0)

    (0) —(function)→ (1)
    (1) —{↘}—(identifier)@name→ (2)
    (2) —{↗¹}—𝜀→ (✓)
    ");
}

#[test]
fn to_string_annotation() {
    insta::assert_snapshot!(snapshot("Q = (identifier) @name ::string"), @r"
    Q = (0)

    (0) —(identifier)—[CaptureNode, ToString]→ (✓)
    ");
}

#[test]
fn anchor_first_child() {
    insta::assert_snapshot!(snapshot("Q = (parent . (child))"), @r"
    Q = (0)

    (0) —(parent)→ (1)
    (1) —{↘.}—(child)→ (2)
    (2) —{↗¹}—𝜀→ (✓)
    ");
}

#[test]
fn anchor_sibling() {
    insta::assert_snapshot!(snapshot("Q = (parent (a) . (b))"), @r"
    Q = (0)

    (0) —(parent)→ (1)
    (1) —{↘}—(a)→ (2)
    (2) —{→·}—(b)→ (3)
    (3) —{↗¹}—𝜀→ (✓)
    ");
}

#[test]
fn optimized_simple() {
    insta::assert_snapshot!(snapshot_optimized("Q = (identifier) @id"), @r"
    Q = (0)

    (0) —(identifier)—[CaptureNode]→ (✓)
    ");
}

#[test]
fn optimized_sequence() {
    insta::assert_snapshot!(snapshot_optimized("Q = { (a) @x (b) @y }"), @r"
    Q = (0)

    (0) —𝜀—[StartObject]→ (1)
    (1) —{→}—(a)—[CaptureNode]→ (2)
    (2) —𝜀—[Field(x)]→ (3)
    (3) —{→}—(b)—[CaptureNode]→ (6)
    (6) —𝜀—[Field(y), EndObject]→ (✓)
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
    Foo = (0)
    Bar = (1)
    Baz = (3)

    (0) —(identifier)→ (✓)
    (1) —<Foo>—𝜀→ (0), (2)
    (2) —𝜀—<Foo>→ (✓)
    (3) —<Bar>—𝜀→ (1), (4)
    (4) —𝜀—<Bar>→ (✓)
    ");
}
