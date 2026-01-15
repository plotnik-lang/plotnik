//! Alternation parsing tests.

use crate::shot_cst;

#[test]
fn alternation() {
    shot_cst!(r#"
        Q = [(identifier) (string)]
    "#);
}

#[test]
fn alternation_with_anonymous() {
    shot_cst!(r#"
        Q = ["true" "false"]
    "#);
}

#[test]
fn alternation_with_capture() {
    shot_cst!(r#"
        Q = [(identifier) (string)] @value
    "#);
}

#[test]
fn alternation_with_quantifier() {
    shot_cst!(r#"
        Q = [
          (identifier)
          (string)* @strings
        ]
    "#);
}

#[test]
fn alternation_nested() {
    shot_cst!(r#"
        Q = (expr
            [(binary) (unary)]
        )
    "#);
}

#[test]
fn alternation_in_field() {
    shot_cst!(r#"
        Q = (call
            arguments: [(string) (number)]
        )
    "#);
}

#[test]
fn unlabeled_alternation_three_items() {
    shot_cst!(r#"
        Q = [(identifier) (number) (string)]
    "#);
}

#[test]
fn tagged_alternation_simple() {
    shot_cst!(r#"
        Q = [
            Ident: (identifier)
            Num: (number)
        ]
    "#);
}

#[test]
fn tagged_alternation_single_line() {
    shot_cst!(r#"
        Q = [A: (a) B: (b) C: (c)]
    "#);
}

#[test]
fn tagged_alternation_with_captures() {
    shot_cst!(r#"
        Q = [
            Assign: (assignment_expression left: (identifier) @left)
            Call: (call_expression function: (identifier) @func)
        ] @stmt
    "#);
}

#[test]
fn tagged_alternation_with_type_annotation() {
    shot_cst!(r#"
        Q = [
            Base: (identifier) @name
            Access: (member_expression object: (_) @obj)
        ] @chain :: MemberChain
    "#);
}

#[test]
fn tagged_alternation_nested() {
    shot_cst!(r#"
        Q = (expr
            [
                Binary: (binary_expression)
                Unary: (unary_expression)
            ])
    "#);
}

#[test]
fn tagged_alternation_in_named_def() {
    shot_cst!(r#"
        Statement = [
            Assign: (assignment_expression)
            Call: (call_expression)
            Return: (return_statement)
        ]
    "#);
}

#[test]
fn tagged_alternation_with_quantifier() {
    shot_cst!(r#"
        Q = [
            Single: (statement)
            Multiple: (statement)+
        ]
    "#);
}

#[test]
fn tagged_alternation_with_sequence() {
    shot_cst!(r#"
        Q = [
            Pair: {(key) (value)}
            Single: (value)
        ]
    "#);
}

#[test]
fn tagged_alternation_with_nested_alternation() {
    shot_cst!(r#"
        Q = [
            Literal: [(number) (string)]
            Ident: (identifier)
        ]
    "#);
}

#[test]
fn tagged_alternation_full_example() {
    shot_cst!(r#"
        Expression = [
            Ident: (identifier) @name :: string
            Num: (number) @value :: string
            Str: (string) @value :: string
            Binary: (binary_expression
                left: (Expression) @left
                right: (Expression) @right)
        ]
    "#);
}
