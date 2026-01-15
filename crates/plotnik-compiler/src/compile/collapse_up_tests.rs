//! Unit tests for the Up-collapse optimization pass.

use plotnik_bytecode::Nav;

use super::collapse_up::collapse_up;
use super::CompileResult;
use crate::bytecode::{InstructionIR, Label, MatchIR};

#[test]
fn collapse_up_single_mode() {
    // Up(1) → Up(1) → exit should become Up(2) → exit
    let mut result = CompileResult {
        instructions: vec![
            MatchIR::at(Label(0)).nav(Nav::Up(1)).next(Label(1)).into(),
            MatchIR::at(Label(1)).nav(Nav::Up(1)).next(Label(2)).into(),
            MatchIR::terminal(Label(2)).into(),
        ],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    collapse_up(&mut result);

    // Should collapse to 2 instructions: Up(2) and terminal
    assert_eq!(result.instructions.len(), 2);

    let InstructionIR::Match(m) = &result.instructions[0] else {
        panic!("expected Match");
    };
    assert_eq!(m.nav, Nav::Up(2));
    assert_eq!(m.successors, vec![Label(2)]);
}

#[test]
fn collapse_up_chain_of_three() {
    // Up(1) → Up(2) → Up(3) should become Up(6)
    let mut result = CompileResult {
        instructions: vec![
            MatchIR::at(Label(0)).nav(Nav::Up(1)).next(Label(1)).into(),
            MatchIR::at(Label(1)).nav(Nav::Up(2)).next(Label(2)).into(),
            MatchIR::at(Label(2)).nav(Nav::Up(3)).next(Label(3)).into(),
            MatchIR::terminal(Label(3)).into(),
        ],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    collapse_up(&mut result);

    assert_eq!(result.instructions.len(), 2);

    let InstructionIR::Match(m) = &result.instructions[0] else {
        panic!("expected Match");
    };
    assert_eq!(m.nav, Nav::Up(6));
}

#[test]
fn collapse_up_mixed_modes_no_merge() {
    // Up(1) → UpSkipTrivia(1) should NOT merge (different modes)
    let mut result = CompileResult {
        instructions: vec![
            MatchIR::at(Label(0)).nav(Nav::Up(1)).next(Label(1)).into(),
            MatchIR::at(Label(1))
                .nav(Nav::UpSkipTrivia(1))
                .next(Label(2))
                .into(),
            MatchIR::terminal(Label(2)).into(),
        ],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    collapse_up(&mut result);

    // Should stay 3 instructions
    assert_eq!(result.instructions.len(), 3);
}

#[test]
fn collapse_up_skip_trivia_same_mode() {
    // UpSkipTrivia(1) → UpSkipTrivia(1) should merge
    let mut result = CompileResult {
        instructions: vec![
            MatchIR::at(Label(0))
                .nav(Nav::UpSkipTrivia(1))
                .next(Label(1))
                .into(),
            MatchIR::at(Label(1))
                .nav(Nav::UpSkipTrivia(1))
                .next(Label(2))
                .into(),
            MatchIR::terminal(Label(2)).into(),
        ],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    collapse_up(&mut result);

    assert_eq!(result.instructions.len(), 2);

    let InstructionIR::Match(m) = &result.instructions[0] else {
        panic!("expected Match");
    };
    assert_eq!(m.nav, Nav::UpSkipTrivia(2));
}

#[test]
fn collapse_up_exact_same_mode() {
    // UpExact(1) → UpExact(1) should merge
    let mut result = CompileResult {
        instructions: vec![
            MatchIR::at(Label(0))
                .nav(Nav::UpExact(1))
                .next(Label(1))
                .into(),
            MatchIR::at(Label(1))
                .nav(Nav::UpExact(1))
                .next(Label(2))
                .into(),
            MatchIR::terminal(Label(2)).into(),
        ],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    collapse_up(&mut result);

    assert_eq!(result.instructions.len(), 2);

    let InstructionIR::Match(m) = &result.instructions[0] else {
        panic!("expected Match");
    };
    assert_eq!(m.nav, Nav::UpExact(2));
}

#[test]
fn collapse_up_with_effects_no_merge() {
    // Up(1) with post_effects → Up(1) should NOT merge
    use crate::bytecode::EffectIR;
    use plotnik_bytecode::EffectOpcode;

    let mut result = CompileResult {
        instructions: vec![
            MatchIR::at(Label(0)).nav(Nav::Up(1)).next(Label(1)).into(),
            MatchIR::at(Label(1))
                .nav(Nav::Up(1))
                .post_effects(vec![EffectIR::simple(EffectOpcode::Null, 0)])
                .next(Label(2))
                .into(),
            MatchIR::terminal(Label(2)).into(),
        ],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    collapse_up(&mut result);

    // Should stay 3 instructions (effectful Up can't be absorbed)
    assert_eq!(result.instructions.len(), 3);
}

#[test]
fn collapse_up_max_63() {
    // Up(60) → Up(10) should become Up(63) (capped)
    let mut result = CompileResult {
        instructions: vec![
            MatchIR::at(Label(0)).nav(Nav::Up(60)).next(Label(1)).into(),
            MatchIR::at(Label(1)).nav(Nav::Up(10)).next(Label(2)).into(),
            MatchIR::terminal(Label(2)).into(),
        ],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    collapse_up(&mut result);

    // Capped at 63, remaining Up(7) stays separate
    assert_eq!(result.instructions.len(), 2);

    let InstructionIR::Match(m) = &result.instructions[0] else {
        panic!("expected Match");
    };
    assert_eq!(m.nav, Nav::Up(63));
}

#[test]
fn collapse_up_branching_no_merge() {
    // Up(1) with multiple successors should NOT merge
    let mut result = CompileResult {
        instructions: vec![
            MatchIR::at(Label(0))
                .nav(Nav::Up(1))
                .next_many(vec![Label(1), Label(2)])
                .into(),
            MatchIR::at(Label(1)).nav(Nav::Up(1)).next(Label(3)).into(),
            MatchIR::at(Label(2)).nav(Nav::Up(1)).next(Label(3)).into(),
            MatchIR::terminal(Label(3)).into(),
        ],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    collapse_up(&mut result);

    // Branching instruction can't merge, but its successors can be processed
    // Label(0) has 2 successors, so it stays as Up(1)
    let InstructionIR::Match(m) = &result.instructions[0] else {
        panic!("expected Match");
    };
    assert_eq!(m.nav, Nav::Up(1));
}

#[test]
fn collapse_up_no_up_unchanged() {
    // Non-Up instructions should pass through unchanged
    let mut result = CompileResult {
        instructions: vec![
            MatchIR::at(Label(0)).nav(Nav::Down).next(Label(1)).into(),
            MatchIR::at(Label(1)).nav(Nav::Next).next(Label(2)).into(),
            MatchIR::terminal(Label(2)).into(),
        ],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    collapse_up(&mut result);

    assert_eq!(result.instructions.len(), 3);
}
