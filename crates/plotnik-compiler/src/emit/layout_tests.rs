//! Layout snapshot tests.
//!
//! Tests verify cache-line aligned layout and gap-filling optimization.

use crate::shot_bytecode;

#[test]
fn single_instruction() {
    shot_bytecode!(r#"
        Test = (identifier) @id
    "#);
}

#[test]
fn linear_chain() {
    shot_bytecode!(r#"
        Test = (array (identifier) @a (number) @b)
    "#);
}

#[test]
fn branch() {
    shot_bytecode!(r#"
        Test = [(identifier) @id (number) @num]
    "#);
}

#[test]
fn call_return() {
    shot_bytecode!(r#"
        Inner = (identifier) @name
        Test = (array (Inner) @item)
    "#);
}

#[test]
fn cache_line_boundary() {
    shot_bytecode!(r#"
        Test = (array
            (identifier) @a
            (identifier) @b
            (identifier) @c
            (identifier) @d
            (identifier) @e
            [(number) @x (string) @y]
        )
    "#);
}

#[test]
fn large_instruction() {
    shot_bytecode!(r#"
        Test = (object
            {(pair) @a (pair) @b (pair) @c (pair) @d}* @items
        )
    "#);
}
