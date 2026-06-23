//! Layout snapshot tests.
//!
//! Tests verify cache-line aligned layout and gap-filling optimization.

use crate::shot_bytecode;

#[test]
fn total_steps_past_u16_does_not_wrap() {
    use super::layout::CacheAligned;
    use crate::compiler::core::ir::{InstructionIR, Label, MatchIR};

    // 70_000 independent terminal matches pack 8-per-block into 70_000 steps,
    // past the u16 ceiling. `total_steps` must report the true u32 count so the
    // emitter's overflow guard is reachable instead of silently wrapping.
    let instrs: Vec<InstructionIR> = (0..70_000u32)
        .map(|i| MatchIR::terminal(Label(i)).into())
        .collect();

    let layout = CacheAligned::layout(&instrs, &[]);

    assert!(
        layout.total_steps() > u16::MAX as u32,
        "total_steps wrapped: {}",
        layout.total_steps()
    );
}

#[test]
fn single_instruction() {
    shot_bytecode!(
        r#"
        Test = (identifier) @id
    "#
    );
}

#[test]
fn linear_chain() {
    shot_bytecode!(
        r#"
        Test = (array (identifier) @a (number) @b)
    "#
    );
}

#[test]
fn branch() {
    shot_bytecode!(
        r#"
        Test = [(identifier) @id (number) @num]
    "#
    );
}

#[test]
fn call_return() {
    shot_bytecode!(
        r#"
        Inner = (identifier) @name
        Test = (array (Inner) @item)
    "#
    );
}

#[test]
fn cache_line_boundary() {
    shot_bytecode!(
        r#"
        Test = (array
            (identifier) @a
            (identifier) @b
            (identifier) @c
            (identifier) @d
            (identifier) @e
            [(number) @x (string) @y]
        )
    "#
    );
}

#[test]
fn large_instruction() {
    shot_bytecode!(
        r#"
        Test = (object
            {(pair) @a (pair) @b (pair) @c (pair) @d}* @items
        )
    "#
    );
}
