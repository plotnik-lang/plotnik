//! Tests for type inference.

use crate::graph::{construct_graph, infer_types};
use crate::parser::Parser;
use crate::parser::lexer::lex;
use std::collections::HashSet;

use super::dump_types;

fn infer(source: &str) -> String {
    let tokens = lex(source);
    let parser = Parser::new(source, tokens);
    let result = parser.parse().expect("parse should succeed");
    let graph = construct_graph(source, &result.root);
    let dead_nodes = HashSet::new();

    let inference = infer_types(&graph, &dead_nodes);
    dump_types(&inference)
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
    Foo → T4

    === Types ===
    T3: Optional <anon> → Node
    T4: Record Foo {
        x: T3
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
