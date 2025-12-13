//! Tests for type inference.

use crate::graph::{construct_graph, infer_types};
use crate::parser::Parser;
use crate::parser::lexer::lex;
use std::collections::HashSet;

fn infer(source: &str) -> String {
    let tokens = lex(source);
    let parser = Parser::new(source, tokens);
    let result = parser.parse().expect("parse should succeed");
    let graph = construct_graph(source, &result.root);
    let dead_nodes = HashSet::new();

    let inference = infer_types(&graph, &dead_nodes);
    inference.dump()
}

#[test]
fn simple_capture() {
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
fn capture_with_string_type() {
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
fn multiple_captures() {
    let result = infer("Foo = (function name: (identifier) @name body: (block) @body)");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        name: Node
        body: Node
    }
    ");
}

#[test]
fn no_captures() {
    let result = infer("Foo = (identifier)");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → Void
    ");
}

#[test]
fn optional_quantifier() {
    let result = infer("Foo = (identifier)? @name");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T4

    === Types ===
    T3: Optional <anon> → Node
    T4: Record Foo {
        name: T3
    }
    ");
}

#[test]
fn star_quantifier() {
    let result = infer("Foo = (identifier)* @names");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T4

    === Types ===
    T3: ArrayStar <anon> → Node
    T4: Record Foo {
        names: T3
    }
    ");
}

#[test]
fn plus_quantifier() {
    let result = infer("Foo = (identifier)+ @names");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T4

    === Types ===
    T3: ArrayStar <anon> → Node
    T4: Record Foo {
        names: T3
    }
    ");
}

#[test]
fn tagged_alternation() {
    let result = infer("Foo = [ Ok: (value) @val  Err: (error) @err ]");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T5

    === Types ===
    T3: Record FooOk {
        val: Node
    }
    T4: Record FooErr {
        err: Node
    }
    T5: Enum Foo {
        Ok: T3
        Err: T4
    }
    ");
}

#[test]
fn untagged_alternation_symmetric() {
    let result = infer("Foo = [ (a) @x  (b) @x ]");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        x: Node
    }
    ");
}

#[test]
fn untagged_alternation_asymmetric() {
    let result = infer("Foo = [ (a) @x  (b) @y ]");
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
fn sequence_capture() {
    let result = infer("Foo = { (a) @x (b) @y } @seq");
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
fn nested_captures() {
    let result = infer("Foo = (outer (inner) @inner) @outer");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        inner: Node
    }
    ");
}

#[test]
fn multiple_definitions() {
    let result = infer(
        r#"
        Func = (function name: (identifier) @name)
        Call = (call function: (identifier) @fn)
    "#,
    );
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Func → T3
    Call → T4

    === Types ===
    T3: Record Func {
        name: Node
    }
    T4: Record Call {
        fn: Node
    }
    ");
}

/// Documents the graph structure for a captured plus quantifier.
/// Used to understand effect ordering for type inference.
#[test]
fn graph_structure_captured_plus() {
    use crate::graph::construct_graph;
    use crate::parser::Parser;
    use crate::parser::lexer::lex;

    let source = "Foo = (identifier)+ @names";
    let tokens = lex(source);
    let parser = Parser::new(source, tokens);
    let result = parser.parse().expect("parse should succeed");
    let graph = construct_graph(source, &result.root);

    let mut out = String::new();
    for (id, node) in graph.iter() {
        out.push_str(&format!("N{}: ", id));
        for effect in &node.effects {
            out.push_str(&format!("{:?} ", effect));
        }
        out.push_str(&format!("→ {:?}\n", node.successors));
    }
    insta::assert_snapshot!(out, @r#"
    N0: CaptureNode → [2]
    N1: StartArray → [0]
    N2: PushElement → [3]
    N3: → [0, 4]
    N4: EndArray → [5]
    N5: Field("names") → []
    "#);
}

/// Documents the graph structure for a tagged alternation.
/// Used to understand variant effect ordering for type inference.
#[test]
fn graph_structure_tagged_alternation() {
    use crate::graph::construct_graph;
    use crate::parser::Parser;
    use crate::parser::lexer::lex;

    let source = "Foo = [ Ok: (value) @val  Err: (error) @err ]";
    let tokens = lex(source);
    let parser = Parser::new(source, tokens);
    let result = parser.parse().expect("parse should succeed");
    let graph = construct_graph(source, &result.root);

    let mut out = String::new();
    for (id, node) in graph.iter() {
        out.push_str(&format!("N{}: ", id));
        for effect in &node.effects {
            out.push_str(&format!("{:?} ", effect));
        }
        out.push_str(&format!("→ {:?}\n", node.successors));
    }
    insta::assert_snapshot!(out, @r#"
    N0: → [2, 6]
    N1: → []
    N2: StartVariant("Ok") → [3]
    N3: CaptureNode → [4]
    N4: Field("val") → [5]
    N5: EndVariant → [1]
    N6: StartVariant("Err") → [7]
    N7: CaptureNode → [8]
    N8: Field("err") → [9]
    N9: EndVariant → [1]
    "#);
}

// =============================================================================
// 1-Level Merge Semantics Tests (ADR-0009)
// =============================================================================

#[test]
fn merge_incompatible_primitives_node_vs_string() {
    // Same field with Node in one branch, String in another
    let result = infer("Foo = [ (a) @val  (b) @val ::string ]");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        val: Node
    }

    === Errors ===
    field `val` in `Foo`: incompatible types [Node, String]
    ");
}

#[test]
fn merge_compatible_same_type_node() {
    // Same field with Node in both branches - should merge without error
    let result = infer("Foo = [ (a) @val  (b) @val ]");
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
fn merge_compatible_same_type_string() {
    // Same field with String in both branches - should merge without error
    let result = infer("Foo = [ (a) @val ::string  (b) @val ::string ]");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        val: String
    }
    ");
}

#[test]
fn merge_asymmetric_fields_become_optional() {
    // Different fields in each branch - both become optional (the feature)
    let result = infer("Foo = [ (a) @x  (b) @y ]");
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
fn merge_mixed_compatible_and_asymmetric() {
    // @common in both branches (compatible), @x and @y asymmetric
    // Note: flat scoping means nested captures propagate to root
    let result = infer("Foo = [ { (a) @common (b) @x }  { (a) @common (c) @y } ]");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T5

    === Types ===
    T3: Optional <anon> → Node
    T4: Optional <anon> → Node
    T5: Record Foo {
        common: Node
        x: T3
        y: T4
    }
    ");
}

#[test]
fn merge_multiple_incompatible_fields_reports_all() {
    // Multiple fields with type mismatches - should report all errors
    let result = infer("Foo = [ (a) @x (b) @y  (c) @x ::string (d) @y ::string ]");
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

    === Errors ===
    field `x` in `Foo`: incompatible types [Node, String]
    field `y` in `Foo`: incompatible types [Node, String]
    ");
}

#[test]
fn merge_three_branches_all_compatible() {
    // Three branches, all with same type - no error
    let result = infer("Foo = [ (a) @val  (b) @val  (c) @val ]");
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
fn merge_three_branches_one_incompatible() {
    // Three branches, one has different type
    let result = infer("Foo = [ (a) @val  (b) @val  (c) @val ::string ]");
    insta::assert_snapshot!(result, @r"
    === Entrypoints ===
    Foo → T3

    === Types ===
    T3: Record Foo {
        val: Node
    }

    === Errors ===
    field `val` in `Foo`: incompatible types [Node, String]
    ");
}
