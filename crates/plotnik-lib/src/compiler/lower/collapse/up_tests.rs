//! Unit tests for the Up-collapse optimization pass.

use crate::bytecode::Nav;

use super::up::collapse_up;
use crate::compiler::lower::ir::NfaGraph;
use crate::compiler::lower::ir::{InstructionIR, Label, MatchIR};

#[test]
fn collapse_up_single_mode() {
    // Up(1) → Up(1) → exit should become Up(2) → exit
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::Up(1))
                .next(Label(1))
                .into(),
            MatchIR::terminal(Label(1))
                .nav(Nav::Up(1))
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
    assert_eq!(m.nav, Nav::Up(2));
    assert_eq!(m.successors, vec![Label(2)]);
}

#[test]
fn collapse_up_chain_of_three() {
    // Up(1) → Up(2) → Up(3) should become Up(6)
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::Up(1))
                .next(Label(1))
                .into(),
            MatchIR::terminal(Label(1))
                .nav(Nav::Up(2))
                .next(Label(2))
                .into(),
            MatchIR::terminal(Label(2))
                .nav(Nav::Up(3))
                .next(Label(3))
                .into(),
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
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::Up(1))
                .next(Label(1))
                .into(),
            MatchIR::terminal(Label(1))
                .nav(Nav::UpSkipTrivia(1))
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
fn collapse_up_skip_trivia_same_mode() {
    // UpSkipTrivia(1) → UpSkipTrivia(1) should merge
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::UpSkipTrivia(1))
                .next(Label(1))
                .into(),
            MatchIR::terminal(Label(1))
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
fn collapse_up_skip_extras_same_mode() {
    // UpSkipExtras(1) → UpSkipExtras(1) should merge
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::UpSkipExtras(1))
                .next(Label(1))
                .into(),
            MatchIR::terminal(Label(1))
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
    assert_eq!(m.nav, Nav::UpSkipExtras(2));
}

#[test]
fn collapse_up_exact_same_mode() {
    // UpExact(1) → UpExact(1) should merge
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::UpExact(1))
                .next(Label(1))
                .into(),
            MatchIR::terminal(Label(1))
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
    use crate::bytecode::EffectKind;
    use crate::compiler::lower::ir::EffectIR;

    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::Up(1))
                .next(Label(1))
                .into(),
            MatchIR::terminal(Label(1))
                .nav(Nav::Up(1))
                .post_effects(vec![EffectIR::literal(EffectKind::Null, 0)])
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
fn collapse_up_merges_up_to_max() {
    // (MAX - 3) → Up(3) sums to exactly Nav::MAX_UP_LEVEL, so the merge is allowed.
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::Up(Nav::MAX_UP_LEVEL - 3))
                .next(Label(1))
                .into(),
            MatchIR::terminal(Label(1))
                .nav(Nav::Up(3))
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
    assert_eq!(m.nav, Nav::Up(Nav::MAX_UP_LEVEL));
}

#[test]
fn collapse_up_refuses_merge_exceeding_max() {
    // A maxed-out ascent plus more would exceed Nav::MAX_UP_LEVEL. Capping would
    // silently drop upward movement, so the merge is refused and both steps remain.
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::Up(Nav::MAX_UP_LEVEL))
                .next(Label(1))
                .into(),
            MatchIR::terminal(Label(1))
                .nav(Nav::Up(10))
                .next(Label(2))
                .into(),
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
    assert_eq!(m.nav, Nav::Up(Nav::MAX_UP_LEVEL));
}

#[test]
fn collapse_up_branching_no_merge() {
    // Up(1) with multiple successors should NOT merge
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::Up(1))
                .successors(vec![Label(1), Label(2)])
                .into(),
            MatchIR::terminal(Label(1))
                .nav(Nav::Up(1))
                .next(Label(3))
                .into(),
            MatchIR::terminal(Label(2))
                .nav(Nav::Up(1))
                .next(Label(3))
                .into(),
            MatchIR::terminal(Label(3)).into(),
        ],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    collapse_up(&mut result);

    let InstructionIR::Match(m) = &result.instructions[0] else {
        panic!("expected Match");
    };
    assert_eq!(m.nav, Nav::Up(1));
}

#[test]
fn collapse_up_deep_chain_splits_without_dangling() {
    // Regression for #455. A same-mode Up run longer than Nav::MAX_UP_LEVEL forces
    // the head to stop mid-chain. The absorbed node at that boundary used to be
    // reprocessed as a fresh merge start, re-absorbing (and removing) the live
    // boundary node the head now points at — dangling the head's successor.
    const DEPTH: u32 = 130;

    let mut instructions: Vec<InstructionIR> = (0..DEPTH)
        .map(|i| {
            MatchIR::terminal(Label(i))
                .nav(Nav::Up(1))
                .next(Label(i + 1))
                .into()
        })
        .collect();
    instructions.push(MatchIR::terminal(Label(DEPTH)).into());

    let mut result = NfaGraph {
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
            assert!(
                n <= Nav::MAX_UP_LEVEL,
                "Up({n}) exceeds the encodable level"
            );
            total += u32::from(n);
        }
    }
    assert_eq!(
        total, DEPTH,
        "ascent depth must be preserved across the split"
    );
}

/// A deep run of same-mode *constraint* Ups splits at the encoding cap exactly
/// like plain `Up`: contiguous, no dangling edge, total ascent preserved. Sound
/// because `Up*` composes — the VM checks the constraint at every exited level,
/// so the split runs partition the levels with no gap (#471).
fn assert_constraint_chain_splits(make: fn(u8) -> Nav) {
    const DEPTH: u32 = 130;

    let mut instructions: Vec<InstructionIR> = (0..DEPTH)
        .map(|i| {
            MatchIR::terminal(Label(i))
                .nav(make(1))
                .next(Label(i + 1))
                .into()
        })
        .collect();
    instructions.push(MatchIR::terminal(Label(DEPTH)).into());

    let mut result = NfaGraph {
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
            && let Some(n) = m.nav.up_level()
        {
            assert_eq!(
                m.nav.up_mode_tag(),
                make(1).up_mode_tag(),
                "split changed the Up mode"
            );
            assert!(
                n <= Nav::MAX_UP_LEVEL,
                "level {n} exceeds the encodable cap {}",
                Nav::MAX_UP_LEVEL
            );
            total += u32::from(n);
        }
    }
    assert_eq!(
        total, DEPTH,
        "ascent depth must be preserved across the split"
    );
}

#[test]
fn collapse_up_skip_trivia_deep_chain_splits() {
    assert_constraint_chain_splits(Nav::UpSkipTrivia);
}

#[test]
fn collapse_up_skip_extras_deep_chain_splits() {
    assert_constraint_chain_splits(Nav::UpSkipExtras);
}

#[test]
fn collapse_up_exact_deep_chain_splits() {
    assert_constraint_chain_splits(Nav::UpExact);
}

#[test]
fn collapse_up_no_up_unchanged() {
    // Non-Up instructions should pass through unchanged
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::Down)
                .next(Label(1))
                .into(),
            MatchIR::terminal(Label(1))
                .nav(Nav::Next)
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
