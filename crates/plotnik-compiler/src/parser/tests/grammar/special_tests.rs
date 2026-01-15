//! Special node (ERROR, MISSING) parsing tests.

use crate::shot_cst;

#[test]
fn error_node() {
    shot_cst!(r#"
        Q = (ERROR)
    "#);
}

#[test]
fn error_node_with_capture() {
    shot_cst!(r#"
        Q = (ERROR) @err
    "#);
}

#[test]
fn missing_node_bare() {
    shot_cst!(r#"
        Q = (MISSING)
    "#);
}

#[test]
fn missing_node_with_type() {
    shot_cst!(r#"
        Q = (MISSING identifier)
    "#);
}

#[test]
fn missing_node_with_string() {
    shot_cst!(r#"
        Q = (MISSING ";")
    "#);
}

#[test]
fn missing_node_with_capture() {
    shot_cst!(r#"
        Q = (MISSING ";") @missing_semi
    "#);
}

#[test]
fn error_in_alternation() {
    shot_cst!(r#"
        Q = [(ERROR) (identifier)]
    "#);
}

#[test]
fn missing_in_sequence() {
    shot_cst!(r#"
        Q = {(MISSING ";") (identifier)}
    "#);
}

#[test]
fn special_node_nested() {
    shot_cst!(r#"
        Q = (function_definition
            body: (block (ERROR)))
    "#);
}

#[test]
fn error_with_quantifier() {
    shot_cst!(r#"
        Q = (ERROR)*
    "#);
}

#[test]
fn missing_with_quantifier() {
    shot_cst!(r#"
        Q = (MISSING identifier)?
    "#);
}
