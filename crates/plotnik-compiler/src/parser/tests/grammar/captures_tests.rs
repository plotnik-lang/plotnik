//! Capture parsing tests.

use crate::shot_cst;

#[test]
fn capture() {
    shot_cst!(r#"
        Q = (identifier) @name
    "#);
}

#[test]
fn capture_nested() {
    shot_cst!(r#"
        Q = (call function: (identifier) @func)
    "#);
}

#[test]
fn multiple_captures() {
    shot_cst!(r#"
        Q = (binary
            left: (_) @left
            right: (_) @right) @expr
    "#);
}

#[test]
fn capture_with_type_annotation() {
    shot_cst!(r#"
        Q = (identifier) @name :: string
    "#);
}

#[test]
fn capture_with_custom_type() {
    shot_cst!(r#"
        Q = (function_declaration) @fn :: FunctionDecl
    "#);
}

#[test]
fn capture_without_type_annotation() {
    shot_cst!(r#"
        Q = (identifier) @name
    "#);
}

#[test]
fn multiple_captures_with_types() {
    shot_cst!(r#"
        Q = (binary
            left: (_) @left :: Node
            right: (_) @right :: string) @expr :: BinaryExpr
    "#);
}

#[test]
fn sequence_capture_with_type() {
    shot_cst!(r#"
        Q = {(a) (b)} @seq :: MySequence
    "#);
}

#[test]
fn alternation_capture_with_type() {
    shot_cst!(r#"
        Q = [(identifier) (number)] @value :: Value
    "#);
}

#[test]
fn quantified_capture_with_type() {
    shot_cst!(r#"
        Q = (statement)+ @stmts :: Statement
    "#);
}

#[test]
fn nested_captures_with_types() {
    shot_cst!(r#"
        Q = (function
            name: (identifier) @name :: string
            body: (block
                (statement)* @body_stmts :: Statement)) @func :: Function
    "#);
}

#[test]
fn capture_with_type_no_spaces() {
    shot_cst!(r#"
        Q = (identifier) @name::string
    "#);
}

#[test]
fn capture_literal() {
    shot_cst!(r#"
        Q = "foo" @keyword
    "#);
}

#[test]
fn capture_literal_with_type() {
    shot_cst!(r#"
        Q = "return" @kw :: string
    "#);
}

#[test]
fn capture_literal_in_tree() {
    shot_cst!(r#"
        Q = (binary_expression "+" @op)
    "#);
}

#[test]
fn capture_literal_with_quantifier() {
    shot_cst!(r#"
        Q = ","* @commas
    "#);
}
