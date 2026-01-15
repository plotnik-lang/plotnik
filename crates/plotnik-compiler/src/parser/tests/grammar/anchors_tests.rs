//! Anchor parsing tests.

use crate::shot_cst;

#[test]
fn anchor_first_child() {
    shot_cst!(r#"
        Q = (block . (first_statement))
    "#);
}

#[test]
fn anchor_last_child() {
    shot_cst!(r#"
        Q = (block (last_statement) .)
    "#);
}

#[test]
fn anchor_adjacency() {
    shot_cst!(r#"
        Q = (dotted_name (identifier) @a . (identifier) @b)
    "#);
}

#[test]
fn anchor_both_ends() {
    shot_cst!(r#"
        Q = (array . (element) .)
    "#);
}

#[test]
fn anchor_multiple_adjacent() {
    shot_cst!(r#"
        Q = (tuple . (a) . (b) . (c) .)
    "#);
}

#[test]
fn anchor_in_sequence() {
    shot_cst!(r#"
        Q = (parent {. (first) (second) .})
    "#);
}
