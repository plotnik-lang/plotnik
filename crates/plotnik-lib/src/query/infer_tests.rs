//! Tests for type inference.

use indoc::indoc;

use crate::query::Query;

fn infer(source: &str) -> String {
    let query = Query::try_from(source)
        .expect("parse should succeed")
        .build_graph();
    query.type_info().dump()
}

fn infer_with_graph(source: &str) -> String {
    let query = Query::try_from(source)
        .expect("parse should succeed")
        .build_graph();
    let mut out = String::new();
    out.push_str(&query.graph().dump_live(query.dead_nodes()));
    out.push('\n');
    out.push_str(&query.type_info().dump());
    out
}

#[test]
fn debug_star_quantifier_graph() {
    // See graph BEFORE optimization (what type inference actually sees)
    let (query, pre_opt_dump) = Query::try_from("Foo = ((item) @items)*")
        .expect("parse should succeed")
        .build_graph_with_pre_opt_dump();
    let mut out = String::new();
    out.push_str("(pre-optimization)\n");
    out.push_str(&pre_opt_dump);
    out.push_str("\n(post-optimization)\n");
    out.push_str(&query.graph().dump_live(query.dead_nodes()));
    out.push('\n');
    out.push_str(&query.type_info().dump());
    insta::assert_snapshot!(out, @r"
    (pre-optimization)
    Foo = (3)

    (0) —(_)→ (1)
    (1) —{↘}—(item)—[CaptureNode]→ (2)
    (2) —{↗¹}—𝜀→ (5)
    (3) —𝜀—[StartArray]→ (6)
    (4) —𝜀—[EndArray]→ (✓)
    (5) —𝜀—[PushElement]→ (9)
    (6) —𝜀→ (7), (4)
    (7) —𝜀→ (0)
    (8) —{→}—𝜀→ (0)
    (9) —𝜀→ (8), (4)

    (post-optimization)
    Foo = (6)

    (0) —(_)→ (1)
    (1) —{↘}—(item)—[CaptureNode]→ (9)
    (4) —𝜀—[EndArray]→ (✓)
    (6) —𝜀—[StartArray]→ (0), (4)
    (8) —{→}—𝜀→ (0)
    (9) —{↗¹}—𝜀—[PushElement]→ (8), (4)

    Foo = [Node]
    ");
}

#[test]
fn debug_graph_structure() {
    let result = infer_with_graph("Foo = (identifier) @name");
    insta::assert_snapshot!(result, @r"
    Foo = (0)

    (0) —(identifier)—[CaptureNode]→ (✓)

    Foo = Node
    ");
}

#[test]
fn debug_incompatible_types_graph() {
    let input = indoc! {r#"
        Foo = [ (a) @v (b) @v ::string ]
    "#};

    let query = Query::new(input)
        .exec()
        .expect("parse should succeed")
        .build_graph();

    let mut out = String::new();
    out.push_str(&query.graph().dump_live(query.dead_nodes()));
    out.push_str(&format!("\n(dead nodes: {})\n\n", query.dead_nodes().len()));
    out.push_str(&query.type_info().dump());
    insta::assert_snapshot!(out, @r"
    Foo = (0)

    (0) —𝜀—[StartObject]→ (2), (4)
    (2) —(a)—[CaptureNode]→ (3)
    (3) —𝜀—[Field(v)]→ (7)
    (4) —(b)—[CaptureNode, ToString]→ (5)
    (5) —𝜀—[Field(v)]→ (7)
    (7) —𝜀—[EndObject]→ (✓)

    (dead nodes: 2)

    Foo = Node

    Errors:
      field `v` in `Foo`: incompatible types [Node, String]
    ");
}

#[test]
fn single_node_capture() {
    let result = infer("Foo = (identifier) @name");
    insta::assert_snapshot!(result, @"Foo = Node");
}

#[test]
fn string_capture() {
    let result = infer("Foo = (identifier) @name ::string");
    insta::assert_snapshot!(result, @"Foo = str");
}

#[test]
fn multiple_captures_flat() {
    let result = infer("Foo = (a (b) @x (c) @y)");
    insta::assert_snapshot!(result, @r"
    Foo = {
      x: Node
      y: Node
    }
    ");
}

#[test]
fn no_captures_void() {
    let result = infer("Foo = (identifier)");
    insta::assert_snapshot!(result, @"Foo = ()");
}

#[test]
fn captured_sequence_creates_struct() {
    let input = indoc! {r#"
        Foo = { (a) @x (b) @y } @z
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    Foo = FooScope3

    FooScope3 = {
      x: Node
      y: Node
    }
    ");
}

#[test]
fn nested_captured_sequence() {
    let input = indoc! {r#"
        Foo = { (outer) @a { (inner) @b } @nested } @root
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    Foo = FooScope4

    FooScope3 = { b: Node }
    FooScope4 = {
      a: Node
      nested: FooScope3
    }
    ");
}

#[test]
fn sequence_without_capture_propagates() {
    let input = indoc! {r#"
        Foo = { (a) @x (b) @y }
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    Foo = {
      x: Node
      y: Node
    }
    ");
}

#[test]
fn untagged_alternation_symmetric() {
    let input = indoc! {r#"
        Foo = [ (a) @v (b) @v ]
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @"Foo = Node");
}

#[test]
fn untagged_alternation_asymmetric() {
    let input = indoc! {r#"
        Foo = [ (a) @x (b) @y ]
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    Foo = {
      x: Node?
      y: Node?
    }
    ");
}

#[test]
fn tagged_alternation_uncaptured_propagates() {
    let input = indoc! {r#"
        Foo = [ A: (a) @x B: (b) @y ]
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    Foo = {
      A => Node
      B => Node
    }
    ");
}

#[test]
fn tagged_alternation_captured_creates_enum() {
    let input = indoc! {r#"
        Foo = [ A: (a) @x B: (b) @y ] @choice
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    Foo = FooScope3

    FooScope3 = {
      A => Node
      B => Node
    }
    ");
}

#[test]
fn captured_untagged_alternation_creates_struct() {
    let input = indoc! {r#"
        Foo = [ (a) @x (b) @y ] @val
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    Foo = FooScope3

    FooScope3 = {
      x: Node?
      y: Node?
    }
    ");
}

#[test]
fn star_quantifier() {
    let result = infer("Foo = ((item) @items)*");
    insta::assert_snapshot!(result, @"Foo = [Node]");
}

#[test]
fn plus_quantifier() {
    let result = infer("Foo = ((item) @items)+");
    insta::assert_snapshot!(result, @"Foo = [Node]⁺");
}

#[test]
fn optional_quantifier() {
    let result = infer("Foo = ((item) @maybe)?");
    insta::assert_snapshot!(result, @"Foo = Node?");
}

#[test]
fn quantifier_on_sequence() {
    // QIS triggered: ≥2 captures inside quantified expression
    let input = indoc! {r#"
        Foo = { (a) @x (b) @y }*
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    Foo = T4

    FooScope3 = {
      x: Node
      y: Node
    }
    T4 = [FooScope3]
    ");
}

#[test]
fn qis_single_capture_no_trigger() {
    // Single capture inside sequence - no QIS
    // Note: The sequence creates its own scope, so the capture goes there.
    // Without explicit capture on the sequence, the struct is orphaned.
    let input = indoc! {r#"
        Single = { (a) @item }*
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @"Single = [Node]");
}

#[test]
fn qis_alternation_in_sequence() {
    // Alternation with asymmetric captures inside quantified sequence
    // QIS triggered (2 captures), creates element struct
    // Note: Current impl doesn't apply optionality for alternation branches in QIS
    let input = indoc! {r#"
        Foo = { [ (a) @x (b) @y ] }*
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    Foo = T6

    FooScope3 = {
      x: Node?
      y: Node?
    }
    T6 = [FooScope3]
    ");
}

#[test]
fn quantified_seq_with_inline_tagged_alt() {
    // Issue #5: captures from inline tagged alternation inside quantified sequence
    // The tagged alternation is uncaptured, so it should behave like untagged.
    // All captures should propagate with Optional cardinality.
    let input = indoc! {r#"
        Test = { [ A: (a) @x  B: (b) @y ] }* @items
    "#};

    let result = infer_with_graph(input);
    insta::assert_snapshot!(result, @r"
    Test = (14)

    (00) —𝜀—[StartObject]→ (01)
    (01) —{→}—𝜀→ (04), (07)
    (04) —(a)—[StartVariant(A), CaptureNode, CaptureNode]→ (05)
    (05) —𝜀—[EndVariant]→ (17)
    (07) —(b)—[StartVariant(B), CaptureNode, CaptureNode]→ (08)
    (08) —𝜀—[EndVariant]→ (17)
    (10) —𝜀—[EndArray]→ (✓)
    (14) —𝜀—[StartArray]→ (00), (10)
    (16) —{→}—𝜀→ (00)
    (17) —𝜀—[EndObject, PushElement]→ (16), (10)

    Test = T6

    TestScope3 = {
      x: Node?
      y: Node?
    }
    T6 = [TestScope3]
    ");
}

#[test]
fn compatible_types_in_alternation() {
    let input = indoc! {r#"
        Foo = [ (a) @v (b) @v ]
    "#};

    let query = Query::try_from(input).expect("parse").build_graph();
    assert!(query.type_info().errors.is_empty());
}

#[test]
fn incompatible_types_in_alternation() {
    let input = indoc! {r#"
        Foo = [ (a) @v (b) @v ::string ]
    "#};

    let result = infer_with_graph(input);
    insta::assert_snapshot!(result, @r"
    Foo = (0)

    (0) —𝜀—[StartObject]→ (2), (4)
    (2) —(a)—[CaptureNode]→ (3)
    (3) —𝜀—[Field(v)]→ (7)
    (4) —(b)—[CaptureNode, ToString]→ (5)
    (5) —𝜀—[Field(v)]→ (7)
    (7) —𝜀—[EndObject]→ (✓)

    Foo = Node

    Errors:
      field `v` in `Foo`: incompatible types [Node, String]
    ");
}

#[test]
fn multiple_definitions() {
    let input = indoc! {r#"
        Func = (function_declaration name: (identifier) @name)
        Class = (class_declaration name: (identifier) @name body: (class_body) @body)
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    Func = Node

    Class = {
      name: Node
      body: Node
    }
    ");
}

#[test]
fn deeply_nested_node() {
    let input = indoc! {r#"
        Foo = (a (b (c (d) @val)))
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @"Foo = Node");
}

#[test]
fn wildcard_capture() {
    let result = infer("Foo = _ @any");
    insta::assert_snapshot!(result, @"Foo = Node");
}

#[test]
fn string_literal_capture() {
    let result = infer(r#"Foo = "+" @op"#);
    insta::assert_snapshot!(result, @"Foo = Node");
}
