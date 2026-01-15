//! Integration tests for the compilation pipeline.

use crate::shot_bytecode;

#[test]
fn compile_simple_named_node() {
    shot_bytecode!("Test = (identifier)");
}

#[test]
fn compile_alternation() {
    shot_bytecode!("Test = [(identifier) (number)]");
}

#[test]
fn compile_sequence() {
    shot_bytecode!("Test = {(comment) (identifier)}");
}

#[test]
fn compile_quantified() {
    shot_bytecode!("Test = (identifier)*");
}

#[test]
fn compile_capture() {
    shot_bytecode!("Test = (identifier) @id");
}

#[test]
fn compile_nested() {
    shot_bytecode!("Test = (call_expression function: (identifier) @fn)");
}

#[test]
fn compile_large_tagged_alternation() {
    // Regression test: alternations with 30+ branches should compile
    // by splitting epsilon transitions into a cascade.
    shot_bytecode!(r#"
        Q = [
            A0: (identifier) @x0   A1: (identifier) @x1   A2: (identifier) @x2
            A3: (identifier) @x3   A4: (identifier) @x4   A5: (identifier) @x5
            A6: (identifier) @x6   A7: (identifier) @x7   A8: (identifier) @x8
            A9: (identifier) @x9   A10: (identifier) @x10 A11: (identifier) @x11
            A12: (identifier) @x12 A13: (identifier) @x13 A14: (identifier) @x14
            A15: (identifier) @x15 A16: (identifier) @x16 A17: (identifier) @x17
            A18: (identifier) @x18 A19: (identifier) @x19 A20: (identifier) @x20
            A21: (identifier) @x21 A22: (identifier) @x22 A23: (identifier) @x23
            A24: (identifier) @x24 A25: (identifier) @x25 A26: (identifier) @x26
            A27: (identifier) @x27 A28: (identifier) @x28 A29: (identifier) @x29
        ]
    "#);
}

#[test]
fn compile_unlabeled_alternation_5_branches_with_captures() {
    // Regression test: unlabeled alternation with 5+ branches where each has
    // a unique capture requires 8+ pre-effects (4 nulls + 4 sets per branch).
    // This exceeds the 3-bit limit (max 7) and must cascade via epsilon chain.
    shot_bytecode!(
        "Q = [(identifier) @a (number) @b (string) @c (binary_expression) @d (call_expression) @e]"
    );
}

#[test]
fn compile_unlabeled_alternation_8_branches_with_captures() {
    // Even more extreme: 8 branches means 14 pre-effects per branch (7 nulls + 7 sets).
    // This requires 2 cascade steps per branch.
    shot_bytecode!(r#"
        Q = [(identifier) @a (number) @b (string) @c (binary_expression) @d
             (call_expression) @e (member_expression) @f (array) @g (object) @h]
    "#);
}
