//! Unit tests for the Up-collapse optimization pass.

use plotnik_bytecode::Nav;

use super::CompileResult;
use super::collapse_up::collapse_up;
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
fn collapse_up_skip_extras_same_mode_within_encoding_limit() {
    let mut result = CompileResult {
        instructions: vec![
            MatchIR::at(Label(0))
                .nav(Nav::UpSkipExtras(52))
                .next(Label(1))
                .into(),
            MatchIR::at(Label(1))
                .nav(Nav::UpSkipExtras(1))
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
    assert_eq!(m.nav, Nav::UpSkipExtras(53));
}

#[test]
fn collapse_up_skip_extras_does_not_exceed_encoding_limit() {
    let mut result = CompileResult {
        instructions: vec![
            MatchIR::at(Label(0))
                .nav(Nav::UpSkipExtras(52))
                .next(Label(1))
                .into(),
            MatchIR::at(Label(1))
                .nav(Nav::UpSkipExtras(2))
                .next(Label(2))
                .into(),
            MatchIR::terminal(Label(2)).into(),
        ],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    collapse_up(&mut result);

    assert_eq!(result.instructions.len(), 3);
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
fn collapse_up_merges_up_to_max() {
    // Up(60) → Up(3) sums to exactly 63 (the max), so the merge is allowed.
    let mut result = CompileResult {
        instructions: vec![
            MatchIR::at(Label(0)).nav(Nav::Up(60)).next(Label(1)).into(),
            MatchIR::at(Label(1)).nav(Nav::Up(3)).next(Label(2)).into(),
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
    assert_eq!(m.nav, Nav::Up(63));
}

#[test]
fn collapse_up_refuses_merge_exceeding_max() {
    // Up(60) → Up(10) would sum to 70 > 63. Capping to 63 would silently drop 7
    // levels of upward movement, so the merge is refused and both steps remain.
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

    assert_eq!(result.instructions.len(), 3);

    let InstructionIR::Match(m) = &result.instructions[0] else {
        panic!("expected Match");
    };
    assert_eq!(m.nav, Nav::Up(60));
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
fn collapse_up_deep_chain_splits_without_dangling() {
    // Regression for #455. A same-mode Up run longer than the 63-level cap forces
    // the head to stop mid-chain. The absorbed node at that boundary used to be
    // reprocessed as a fresh merge start, re-absorbing (and removing) the live
    // boundary node the head now points at — dangling the head's successor.
    const DEPTH: u32 = 130;

    let mut instructions: Vec<InstructionIR> = (0..DEPTH)
        .map(|i| {
            MatchIR::at(Label(i))
                .nav(Nav::Up(1))
                .next(Label(i + 1))
                .into()
        })
        .collect();
    instructions.push(MatchIR::terminal(Label(DEPTH)).into());

    let mut result = CompileResult {
        instructions,
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    collapse_up(&mut result);

    let present: std::collections::HashSet<Label> =
        result.instructions.iter().map(|i| i.label()).collect();
    let mut total = 0u32;
    for instr in &result.instructions {
        for succ in instr.successors() {
            assert!(present.contains(succ), "dangling successor {succ:?}");
        }
        if let InstructionIR::Match(m) = instr
            && let Nav::Up(n) = m.nav
        {
            assert!(n <= 63, "Up({n}) exceeds the encodable level");
            total += u32::from(n);
        }
    }
    assert_eq!(
        total, DEPTH,
        "ascent depth must be preserved across the split"
    );
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
