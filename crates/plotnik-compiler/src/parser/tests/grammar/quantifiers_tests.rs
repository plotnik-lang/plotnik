//! Quantifier parsing tests.

use crate::shot_cst;

#[test]
fn quantifier_star() {
    shot_cst!(r#"
        Q = (statement)*
    "#);
}

#[test]
fn quantifier_plus() {
    shot_cst!(r#"
        Q = (statement)+
    "#);
}

#[test]
fn quantifier_optional() {
    shot_cst!(r#"
        Q = (statement)?
    "#);
}

#[test]
fn quantifier_with_capture() {
    shot_cst!(r#"
        Q = (statement)* @statements
    "#);
}

#[test]
fn quantifier_inside_node() {
    shot_cst!(r#"
        Q = (block
            (statement)*)
    "#);
}
