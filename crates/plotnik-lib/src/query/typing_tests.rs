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
    N1: ε [Field(v)] [Field(v)] → ∅
    N2: (a) [Capture] → N1
    N4: (b) [Capture] [ToString] → N1

    === Dead nodes count: 2 ===

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
    Foo → Void

    === Types ===
    T3: Record FooScope3 {
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
    Foo → T5

    === Types ===
    T3: Record FooA {
        x: Node
    }
    T4: Record FooB {
        y: Node
    }
    T5: Enum Foo {
        A: T3
        B: T4
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
    Foo → T5

    === Types ===
    T3: Record FooA {
        x: Node
    }
    T4: Record FooB {
        y: Node
    }
    T5: Enum Foo {
        A: T3
        B: T4
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
    T3: ArrayStar <anon> → Node
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
    let input = indoc! {r#"
        Foo = { (a) @x (b) @y }*
    "#};

    let result = infer(input);
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → Void

    === Types ===
    T3: ArrayStar <anon> → Node
    T4: ArrayStar <anon> → Node
    T5: Record FooScope3 {
        x: T3
        y: T4
    }
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
    N1: ε [Field(v)] [Field(v)] → ∅
    N2: (a) [Capture] → N1
    N4: (b) [Capture] [ToString] → N1

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
