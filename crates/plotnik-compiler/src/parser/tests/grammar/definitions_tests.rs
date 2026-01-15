//! Definition parsing tests.

use crate::{shot_cst, shot_error};

#[test]
fn simple_named_def() {
    shot_cst!(r#"
        Expr = (identifier)
    "#);
}

#[test]
fn named_def_with_alternation() {
    shot_cst!(r#"
        Value = [(identifier) (number) (string)]
    "#);
}

#[test]
fn named_def_with_sequence() {
    shot_cst!(r#"
        Pair = {(identifier) (expression)}
    "#);
}

#[test]
fn named_def_with_captures() {
    shot_cst!(r#"
        BinaryOp = (binary_expression
            left: (_) @left
            operator: _ @op
            right: (_) @right)
    "#);
}

#[test]
fn multiple_named_defs() {
    shot_cst!(r#"
        Expr = (expression)
        Stmt = (statement)
    "#);
}

#[test]
fn named_def_then_expression() {
    shot_error!(r#"
        Expr = [(identifier) (number)]
        (program (Expr) @value)
    "#);
}

#[test]
fn named_def_referencing_another() {
    shot_cst!(r#"
        Literal = [(number) (string)]
        Expr = [(identifier) (Literal)]
    "#);
}

#[test]
fn named_def_with_quantifier() {
    shot_cst!(r#"
        Statements = (statement)+
    "#);
}

#[test]
fn named_def_complex_recursive() {
    shot_cst!(r#"
        NestedCall = (call_expression
            function: [(identifier) @name (NestedCall) @inner]
            arguments: (arguments))
    "#);
}

#[test]
fn named_def_with_type_annotation() {
    shot_cst!(r#"
        Func = (function_declaration
            name: (identifier) @name :: string
            body: (_) @body)
    "#);
}

#[test]
fn unnamed_def_allowed_as_last() {
    shot_cst!(r#"
        Expr = (identifier)
        Q = (program (Expr) @value)
    "#);
}
