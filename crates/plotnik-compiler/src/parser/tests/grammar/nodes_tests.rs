//! Node parsing tests.

use crate::shot_cst;

#[test]
fn empty_input() {
    shot_cst!("");
}

#[test]
fn simple_named_node() {
    shot_cst!(r#"
        Q = (identifier)
    "#);
}

#[test]
fn nested_node() {
    shot_cst!(r#"
        Q = (function_definition name: (identifier))
    "#);
}

#[test]
fn deeply_nested() {
    shot_cst!(r#"
        Q = (a
            (b
            (c
                (d))))
    "#);
}

#[test]
fn sibling_children() {
    shot_cst!(r#"
        Q = (block
            (statement)
            (statement)
            (statement))
    "#);
}

#[test]
fn wildcard() {
    shot_cst!(r#"
        Q = (_)
    "#);
}

#[test]
fn anonymous_node() {
    shot_cst!(r#"
        Q = "if"
    "#);
}

#[test]
fn anonymous_node_operator() {
    shot_cst!(r#"
        Q = "+="
    "#);
}

#[test]
fn supertype_basic() {
    shot_cst!(r#"
        Q = (expression/binary_expression)
    "#);
}

#[test]
fn supertype_with_string_subtype() {
    shot_cst!(r#"
        Q = (expression/"()")
    "#);
}

#[test]
fn supertype_with_capture() {
    shot_cst!(r#"
        Q = (expression/binary_expression) @expr
    "#);
}

#[test]
fn supertype_with_children() {
    shot_cst!(r#"
        Q = (expression/binary_expression
            left: (_) @left
            right: (_) @right)
    "#);
}

#[test]
fn supertype_nested() {
    shot_cst!(r#"
        Q = (statement/expression_statement
            (expression/call_expression))
    "#);
}

#[test]
fn supertype_in_alternation() {
    shot_cst!(r#"
        Q = [(expression/identifier) (expression/number)]
    "#);
}

#[test]
fn no_supertype_plain_node() {
    shot_cst!(r#"
        Q = (identifier)
    "#);
}
