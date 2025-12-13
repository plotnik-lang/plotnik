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
    out.push_str("=== Graph ===\n");
    out.push_str(&query.graph().dump_live(query.dead_nodes()));
    out.push_str("\n");
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
    out.push_str("=== Graph (before optimization - what type inference sees) ===\n");
    out.push_str(&pre_opt_dump);
    out.push_str("\n=== Graph (after optimization) ===\n");
    out.push_str(&query.graph().dump_live(query.dead_nodes()));
    out.push_str("\n");
    out.push_str(&query.type_info().dump());
    insta::assert_snapshot!(out, @r"
    === Graph (before optimization - what type inference sees) ===
    Foo = N4

    N0: (_) → N1
    N1: [Down] (item) [Capture] → N2
    N2: ε [Field(items)] → N3
    N3: [Up(1)] ε → N6
    N4: ε [StartArray] → N7
    N5: ε [EndArray] → ∅
    N6: ε [Push] → N7
    N7: ε → N0, N5

    === Graph (after optimization) ===
    Foo = N4

    N0: (_) → N1
    N1: [Down] (item) [Capture] → N2
    N2: ε [Field(items)] → N6
    N4: ε [StartArray] → N7
    N5: ε [EndArray] → ∅
    N6: [Up(1)] ε [Push] → N7
    N7: ε → N0, N5

    === Entrypoints ===
    Foo → T4

    === Types ===
    T3: ArrayStar <anon> → Node
    T4: Record Foo {
        items: T3
    }
    ");
}

#[test]
fn debug_graph_structure() {
    let result = infer_with_graph("Foo = (identifier) @name");
    insta::assert_snapshot!(result, @r"
    === Graph ===
    Foo = N0

    N0: (identifier) [Capture] → N1
    N1: ε [Field(name)] → ∅

    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        name: Node
    }
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
    out.push_str("=== Graph (after optimization) ===\n");
    out.push_str(&query.graph().dump_live(query.dead_nodes()));
    out.push_str("\n=== Dead nodes count: ");
    out.push_str(&query.dead_nodes().len().to_string());
    out.push_str(" ===\n\n");
    out.push_str(&query.type_info().dump());
    insta::assert_snapshot!(out, @r"
    === Graph (after optimization) ===
    Foo = N0

    N0: ε → N2, N4
    N1: ε → ∅
    N2: (a) [Capture] → N3
    N3: ε [Field(v)] → N1
    N4: (b) [Capture] [ToString] → N5
    N5: ε [Field(v)] → N1

    === Dead nodes count: 0 ===

    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        v: Node
    }

    === Errors ===
    field `v` in `Foo`: incompatible types [Node, String]
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Basic captures
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn single_node_capture() {
    let result = infer("Foo = (identifier) @name");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        name: Node
    }
    ");
}

#[test]
fn string_capture() {
    let result = infer("Foo = (identifier) @name ::string");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        name: String
    }
    ");
}

#[test]
fn multiple_captures_flat() {
    let result = infer("Foo = (a (b) @x (c) @y)");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        x: Node
        y: Node
    }
    ");
}

#[test]
fn no_captures_void() {
    let result = infer("Foo = (identifier)");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → Void
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Captured sequences (composite types)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn captured_sequence_creates_struct() {
    let input = indoc! {r#"
        Foo = { (a) @x (b) @y } @z
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T4

    === Types ===
    T3: Record FooScope3 {
        x: Node
        y: Node
    }
    T4: Record Foo {
        z: T3
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
    === Entrypoints ===
    Foo → T5

    === Types ===
    T3: Record FooScope3 {
        b: Node
    }
    T4: Record FooScope4 {
        a: Node
        nested: T3
    }
    T5: Record Foo {
        root: T4
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
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        x: Node
        y: Node
    }
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Alternations
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn untagged_alternation_symmetric() {
    let input = indoc! {r#"
        Foo = [ (a) @v (b) @v ]
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        v: Node
    }
    ");
}

#[test]
fn untagged_alternation_asymmetric() {
    let input = indoc! {r#"
        Foo = [ (a) @x (b) @y ]
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T5

    === Types ===
    T3: Optional <anon> → Node
    T4: Optional <anon> → Node
    T5: Record Foo {
        x: T3
        y: T4
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
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Enum Foo {
        A: Node
        B: Node
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
    === Entrypoints ===
    Foo → T4

    === Types ===
    T3: Enum FooScope3 {
        A: Node
        B: Node
    }
    T4: Record Foo {
        choice: T3
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
    === Entrypoints ===
    Foo → T6

    === Types ===
    T3: Optional <anon> → Node
    T4: Optional <anon> → Node
    T5: Record FooScope3 {
        x: T3
        y: T4
    }
    T6: Record Foo {
        val: T5
    }
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Quantifiers
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn star_quantifier() {
    let result = infer("Foo = ((item) @items)*");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T4

    === Types ===
    T3: ArrayStar <anon> → Node
    T4: Record Foo {
        items: T3
    }
    ");
}

#[test]
fn plus_quantifier() {
    let result = infer("Foo = ((item) @items)+");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T4

    === Types ===
    T3: ArrayPlus <anon> → Node
    T4: Record Foo {
        items: T3
    }
    ");
}

#[test]
fn optional_quantifier() {
    let result = infer("Foo = ((item) @maybe)?");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T4

    === Types ===
    T3: Optional <anon> → Node
    T4: Record Foo {
        maybe: T3
    }
    ");
}

#[test]
fn quantifier_on_sequence() {
    // QIS triggered: ≥2 captures inside quantified expression
    let input = indoc! {r#"
        Foo = { (a) @x (b) @y }*
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T4

    === Types ===
    T3: Record FooScope3 {
        x: Node
        y: Node
    }
    T4: ArrayStar <anon> → T3
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// QIS: Additional cases from ADR-0009
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn qis_single_capture_no_trigger() {
    // Single capture inside sequence - no QIS
    // Note: The sequence creates its own scope, so the capture goes there.
    // Without explicit capture on the sequence, the struct is orphaned.
    let input = indoc! {r#"
        Single = { (a) @item }*
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Single → T4

    === Types ===
    T3: ArrayStar <anon> → Node
    T4: Record Single {
        item: T3
    }
    ");
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
    === Entrypoints ===
    Foo → T6

    === Types ===
    T3: Optional <anon> → Node
    T4: Optional <anon> → Node
    T5: Record FooScope3 {
        x: T3
        y: T4
    }
    T6: ArrayStar <anon> → T5
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Type compatibility
// ─────────────────────────────────────────────────────────────────────────────

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
    === Graph ===
    Foo = N0

    N0: ε → N2, N4
    N1: ε → ∅
    N2: (a) [Capture] → N3
    N3: ε [Field(v)] → N1
    N4: (b) [Capture] [ToString] → N5
    N5: ε [Field(v)] → N1

    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        v: Node
    }

    === Errors ===
    field `v` in `Foo`: incompatible types [Node, String]
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Multiple definitions
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn multiple_definitions() {
    let input = indoc! {r#"
        Func = (function_declaration name: (identifier) @name)
        Class = (class_declaration name: (identifier) @name body: (class_body) @body)
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Func → T3
    Class → T4

    === Types ===
    T3: Record Func {
        name: Node
    }
    T4: Record Class {
        name: Node
        body: Node
    }
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Edge cases
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn deeply_nested_node() {
    let input = indoc! {r#"
        Foo = (a (b (c (d) @val)))
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        val: Node
    }
    ");
}

#[test]
fn wildcard_capture() {
    let result = infer("Foo = _ @any");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        any: Node
    }
    ");
}

#[test]
fn string_literal_capture() {
    let result = infer(r#"Foo = "+" @op"#);
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        op: Node
    }
    ");
}
