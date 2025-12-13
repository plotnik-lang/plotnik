//! Tests for AST-to-graph construction.

use crate::graph::BuildGraph;
use crate::parser::Parser;
use crate::parser::lexer::lex;

use super::construct_graph;

fn parse_and_construct(source: &str) -> BuildGraph<'_> {
    let tokens = lex(source);
    let parser = Parser::new(source, tokens);
    let result = parser.parse().expect("parse should succeed");
    construct_graph(source, &result.root)
}

// ─────────────────────────────────────────────────────────────────────────────
// Basic Expressions
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn simple_named_node() {
    let g = parse_and_construct("Foo = (identifier)");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: (identifier) → ∅
    ");
}

#[test]
fn anonymous_string() {
    let g = parse_and_construct(r#"Op = "+""#);

    insta::assert_snapshot!(g.dump(), @r#"
    Op = N0

    N0: "+" → ∅
    "#);
}

#[test]
fn wildcard() {
    let g = parse_and_construct("Any = (_)");

    insta::assert_snapshot!(g.dump(), @r"
    Any = N0

    N0: _ → ∅
    ");
}

#[test]
fn wildcard_underscore_literal() {
    let g = parse_and_construct("Any = _");

    insta::assert_snapshot!(g.dump(), @r"
    Any = N0

    N0: _ → ∅
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Nested Nodes
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn nested_node() {
    let g = parse_and_construct("Foo = (call (identifier))");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: (call) → N1
    N1: [Down] (identifier) → N2
    N2: [Up(1)] ε → ∅
    ");
}

#[test]
fn deeply_nested() {
    let g = parse_and_construct("Foo = (a (b (c)))");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: (a) → N1
    N1: [Down] (b) → N2
    N2: [Down] (c) → N3
    N3: [Up(1)] ε → N4
    N4: [Up(1)] ε → ∅
    ");
}

#[test]
fn sibling_nodes() {
    let g = parse_and_construct("Foo = (call (identifier) (arguments))");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: (call) → N1
    N1: [Down] (identifier) → N2
    N2: [Next] (arguments) → N3
    N3: [Up(1)] ε → ∅
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Anchors
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn anchor_first_child() {
    // . before first child → DownSkipTrivia
    let g = parse_and_construct("Foo = (block . (statement))");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: (block) → N1
    N1: [Down.] (statement) → N2
    N2: [Up(1)] ε → ∅
    ");
}

#[test]
fn anchor_last_child() {
    // . after last child → UpSkipTrivia
    let g = parse_and_construct("Foo = (block (statement) .)");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: (block) → N1
    N1: [Down] (statement) → N2
    N2: [Up.(1)] ε → ∅
    ");
}

#[test]
fn anchor_adjacent_siblings() {
    // . between siblings → NextSkipTrivia
    let g = parse_and_construct("Foo = (block (a) . (b))");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: (block) → N1
    N1: [Down] (a) → N2
    N2: [Next.] (b) → N3
    N3: [Up(1)] ε → ∅
    ");
}

#[test]
fn anchor_both_ends() {
    // . at start and end
    let g = parse_and_construct("Foo = (array . (element) .)");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: (array) → N1
    N1: [Down.] (element) → N2
    N2: [Up.(1)] ε → ∅
    ");
}

#[test]
fn anchor_string_literal_first() {
    // . before string literal → DownExact
    let g = parse_and_construct(r#"Foo = (pair . ":" (value))"#);

    insta::assert_snapshot!(g.dump(), @r#"
    Foo = N0

    N0: (pair) → N1
    N1: [Down!] ":" → N2
    N2: [Next] (value) → N3
    N3: [Up(1)] ε → ∅
    "#);
}

#[test]
fn anchor_string_literal_adjacent() {
    // . after string literal before node → NextExact on string, but string is prev
    // Actually the anchor affects the FOLLOWING node, so ":" has Down, "=" has Next!
    let g = parse_and_construct(r#"Foo = (assignment (id) "=" . (value))"#);

    insta::assert_snapshot!(g.dump(), @r#"
    Foo = N0

    N0: (assignment) → N1
    N1: [Down] (id) → N2
    N2: [Next] "=" → N3
    N3: [Next.] (value) → N4
    N4: [Up(1)] ε → ∅
    "#);
}

#[test]
fn anchor_string_literal_last() {
    // . after string literal at end → UpExact
    let g = parse_and_construct(r#"Foo = (semi (stmt) ";" .)"#);

    insta::assert_snapshot!(g.dump(), @r#"
    Foo = N0

    N0: (semi) → N1
    N1: [Down] (stmt) → N2
    N2: [Next] ";" → N3
    N3: [Up!(1)] ε → ∅
    "#);
}

// ─────────────────────────────────────────────────────────────────────────────
// Fields
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn field_constraint() {
    let g = parse_and_construct("Foo = (call name: (identifier))");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: (call) → N1
    N1: [Down] (identifier) @name → N2
    N2: [Up(1)] ε → ∅
    ");
}

#[test]
fn negated_field() {
    let g = parse_and_construct("Foo = (call !arguments)");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: (call) !arguments → ∅
    ");
}

#[test]
fn multiple_negated_fields() {
    let g = parse_and_construct("Foo = (call !arguments !type_arguments)");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: (call) !arguments !type_arguments → ∅
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Sequences
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn sequence_expr() {
    let g = parse_and_construct("Foo = { (a) (b) }");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: ε [StartObj] → N1
    N1: [Next] (a) → N2
    N2: [Next] (b) → N3
    N3: ε [EndObj] → ∅
    ");
}

#[test]
fn empty_sequence() {
    let g = parse_and_construct("Foo = { }");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: ε [StartObj] → N1
    N1: ε → N2
    N2: ε [EndObj] → ∅
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Alternations
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn untagged_alternation() {
    let g = parse_and_construct("Foo = [(identifier) (number)]");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: ε → N2, N3
    N1: ε → ∅
    N2: (identifier) → N1
    N3: (number) → N1
    ");
}

#[test]
fn tagged_alternation() {
    let g = parse_and_construct("Foo = [Ident: (identifier) Num: (number)]");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: ε → N2, N5
    N1: ε → ∅
    N2: ε [Variant(Ident)] → N3
    N3: (identifier) → N4
    N4: ε [EndVariant] → N1
    N5: ε [Variant(Num)] → N6
    N6: (number) → N7
    N7: ε [EndVariant] → N1
    ");
}

#[test]
fn single_branch_alt() {
    let g = parse_and_construct("Foo = [(identifier)]");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: ε → N2
    N1: ε → ∅
    N2: (identifier) → N1
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Captures
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn simple_capture() {
    let g = parse_and_construct("Foo = (identifier) @name");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: (identifier) [Capture] → N1
    N1: ε [Field(name)] → ∅
    ");
}

#[test]
fn capture_with_string_type() {
    let g = parse_and_construct("Foo = (identifier) @name ::string");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: (identifier) [Capture] [ToString] → N1
    N1: ε [Field(name)] → ∅
    ");
}

#[test]
fn nested_capture() {
    let g = parse_and_construct("Foo = (call name: (identifier) @fn_name)");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N0

    N0: (call) → N1
    N1: [Down] (identifier) @name [Capture] → N2
    N2: ε [Field(fn_name)] → N3
    N3: [Up(1)] ε → ∅
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Quantifiers
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn zero_or_more() {
    let g = parse_and_construct("Foo = (identifier)*");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N1

    N0: (identifier) → N3
    N1: ε [StartArray] → N2
    N2: ε → N0, N4
    N3: ε [Push] → N2
    N4: ε [EndArray] → ∅
    ");
}

#[test]
fn one_or_more() {
    let g = parse_and_construct("Foo = (identifier)+");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N1

    N0: (identifier) → N2
    N1: ε [StartArray] → N0
    N2: ε [Push] → N3
    N3: ε → N0, N4
    N4: ε [EndArray] → ∅
    ");
}

#[test]
fn optional() {
    let g = parse_and_construct("Foo = (identifier)?");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N1

    N0: (identifier) → N2
    N1: ε → N0, N2
    N2: ε → ∅
    ");
}

#[test]
fn lazy_zero_or_more() {
    let g = parse_and_construct("Foo = (identifier)*?");

    insta::assert_snapshot!(g.dump(), @r"
    Foo = N1

    N0: (identifier) → N3
    N1: ε [StartArray] → N2
    N2: ε → N4, N0
    N3: ε [Push] → N2
    N4: ε [EndArray] → ∅
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// References
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn simple_reference() {
    let g = parse_and_construct(
        "
        Ident = (identifier)
        Foo = (call (Ident))
        ",
    );

    insta::assert_snapshot!(g.dump(), @r"
    Ident = N0
    Foo = N1

    N0: (identifier) → ∅
    N1: (call) → N2
    N2: [Down] ε +Enter(0, Ident) → N0, N4
    N3: ε +Exit(0) → N4
    N4: [Up(1)] ε → ∅
    ");
}

#[test]
fn multiple_references() {
    let g = parse_and_construct(
        "
        Expr = [(identifier) (number)]
        Foo = (binary left: (Expr) right: (Expr))
        ",
    );

    insta::assert_snapshot!(g.dump(), @r"
    Expr = N0
    Foo = N4

    N0: ε → N2, N3
    N1: ε → ∅
    N2: (identifier) → N1
    N3: (number) → N1
    N4: (binary) → N5
    N5: [Down] ε +Enter(0, Expr) → N0, N7
    N6: ε +Exit(0) → N7
    N7: [Next] ε +Enter(1, Expr) → N0, N9
    N8: ε +Exit(1) → N9
    N9: [Up(1)] ε → ∅
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Multiple Definitions
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn multiple_definitions() {
    let g = parse_and_construct(
        "
        Ident = (identifier)
        Num = (number)
        Str = (string)
        ",
    );

    insta::assert_snapshot!(g.dump(), @r"
    Ident = N0
    Num = N1
    Str = N2

    N0: (identifier) → ∅
    N1: (number) → ∅
    N2: (string) → ∅
    ");
}

// ─────────────────────────────────────────────────────────────────────────────
// Complex Examples
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn function_pattern() {
    let g = parse_and_construct(
        "
        Func = (function_definition
            name: (identifier) @name
            parameters: (parameters (identifier)* @params)
            body: (block))
        ",
    );

    insta::assert_snapshot!(g.dump(), @r"
    Func = N0

    N0: (function_definition) → N1
    N1: [Down] (identifier) @name [Capture] → N2
    N2: ε [Field(name)] → N3
    N3: [Next] (parameters) @parameters → N5
    N4: [Down] (identifier) [Capture] → N7
    N5: ε [StartArray] → N6
    N6: ε → N4, N8
    N7: ε [Push] → N6
    N8: ε [EndArray] → N9
    N9: ε [Field(params)] → N10
    N10: [Up(1)] ε → N11
    N11: [Next] (block) @body → N12
    N12: [Up(1)] ε → ∅
    ");
}

#[test]
fn binary_expression_pattern() {
    let g = parse_and_construct(
        r#"
        BinOp = (binary_expression
            left: (_) @left
            operator: ["+" "-" "*" "/"] @op ::string
            right: (_) @right)
        "#,
    );

    insta::assert_snapshot!(g.dump(), @r#"
    BinOp = N0

    N0: (binary_expression) → N1
    N1: [Down] _ @left [Capture] → N2
    N2: ε [Field(left)] → N3
    N3: [Next] ε → N5, N6, N7, N8
    N4: ε → N9
    N5: "+" [Capture] [ToString] → N4
    N6: "-" [Capture] [ToString] → N4
    N7: "*" [Capture] [ToString] → N4
    N8: "/" [Capture] [ToString] → N4
    N9: ε [Field(op)] → N10
    N10: [Next] _ @right [Capture] → N11
    N11: ε [Field(right)] → N12
    N12: [Up(1)] ε → ∅
    "#);
}
