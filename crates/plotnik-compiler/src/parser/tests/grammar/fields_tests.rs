//! Field parsing tests.

use crate::shot_cst;

#[test]
fn field_expression() {
    shot_cst!(r#"
        Q = (call function: (identifier))
    "#);
}

#[test]
fn multiple_fields() {
    shot_cst!(r#"
        Q = (assignment
            left: (identifier)
            right: (expression))
    "#);
}

#[test]
fn negated_field() {
    shot_cst!(r#"
        Q = (function -async)
    "#);
}

#[test]
fn negated_and_regular_fields() {
    shot_cst!(r#"
        Q = (function
            -async
            name: (identifier))
    "#);
}

#[test]
fn mixed_children_and_fields() {
    shot_cst!(r#"
        Q = (if
            condition: (expr)
            (then_block)
            else: (else_block))
    "#);
}

#[test]
fn fields_and_quantifiers() {
    shot_cst!(r#"
        Q = (node
            foo: (foo)?
            foo: (foo)??
            bar: (bar)*
            bar: (bar)*?
            baz: (baz)+?
            baz: (baz)+?)
    "#);
}

#[test]
fn fields_with_quantifiers_and_captures() {
    shot_cst!(r#"
        Q = (node foo: (bar)* @baz)
    "#);
}
