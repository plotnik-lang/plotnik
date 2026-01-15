//! Unit tests for the lowering pass.

use plotnik_bytecode::{EffectOpcode, Nav, MAX_MATCH_PAYLOAD_SLOTS, MAX_PRE_EFFECTS};

use super::lower::lower;
use super::CompileResult;
use crate::bytecode::{EffectIR, InstructionIR, Label, MatchIR};

const MAX_POST_EFFECTS: usize = 7;
const MAX_NEG_FIELDS: usize = 7;

fn make_effect(_idx: u16) -> EffectIR {
    EffectIR::simple(EffectOpcode::Null, 0)
}

#[test]
fn lower_no_overflow_unchanged() {
    let mut result = CompileResult {
        instructions: vec![MatchIR::at(Label(0))
            .pre_effects((0..3).map(make_effect))
            .next(Label(1))
            .into()],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    lower(&mut result);

    assert_eq!(result.instructions.len(), 1);
}

#[test]
fn lower_pre_effects_overflow() {
    let mut result = CompileResult {
        instructions: vec![MatchIR::at(Label(0))
            .nav(Nav::Down)
            .pre_effects((0..10).map(make_effect))
            .next(Label(1))
            .into()],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    lower(&mut result);

    // Should split into: epsilon chain (2 steps) + actual match
    assert!(result.instructions.len() >= 2);

    // First instruction should be epsilon with effects
    let first = result.instructions.first().unwrap();
    if let InstructionIR::Match(m) = first {
        assert!(m.nav == Nav::Epsilon);
        assert!(m.pre_effects.len() <= MAX_PRE_EFFECTS);
    } else {
        panic!("expected Match");
    }
}

#[test]
fn lower_post_effects_overflow() {
    let mut result = CompileResult {
        instructions: vec![MatchIR::at(Label(0))
            .nav(Nav::Down)
            .post_effects((0..10).map(make_effect))
            .next(Label(1))
            .into()],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    lower(&mut result);

    // Should split: match + epsilon chain for overflow post_effects
    assert!(result.instructions.len() >= 2);

    // All post_effects should be within limits
    for instr in &result.instructions {
        if let InstructionIR::Match(m) = instr {
            assert!(
                m.post_effects.len() <= MAX_POST_EFFECTS,
                "post_effects {} > {}",
                m.post_effects.len(),
                MAX_POST_EFFECTS
            );
        }
    }
}

#[test]
fn lower_neg_fields_overflow() {
    let mut result = CompileResult {
        instructions: vec![MatchIR::at(Label(0))
            .nav(Nav::Down)
            .neg_fields(0..10)
            .next(Label(1))
            .into()],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    lower(&mut result);

    // Should split: match + epsilon chain for overflow neg_fields
    assert!(result.instructions.len() >= 2);

    // All neg_fields should be within limits
    for instr in &result.instructions {
        if let InstructionIR::Match(m) = instr {
            assert!(
                m.neg_fields.len() <= MAX_NEG_FIELDS,
                "neg_fields {} > {}",
                m.neg_fields.len(),
                MAX_NEG_FIELDS
            );
        }
    }
}

#[test]
fn lower_successors_overflow() {
    let succs: Vec<_> = (1..=35).map(Label).collect();
    let mut result = CompileResult {
        instructions: vec![MatchIR::at(Label(0)).next_many(succs).into()],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    lower(&mut result);

    // Should cascade into multiple epsilon instructions
    assert!(result.instructions.len() >= 2);

    // All successors should be within limits
    for instr in &result.instructions {
        if let InstructionIR::Match(m) = instr {
            assert!(
                m.successors.len() <= MAX_MATCH_PAYLOAD_SLOTS,
                "successors {} > {}",
                m.successors.len(),
                MAX_MATCH_PAYLOAD_SLOTS
            );
        }
    }
}

#[test]
fn lower_combined_overflow() {
    let mut result = CompileResult {
        instructions: vec![MatchIR::at(Label(0))
            .nav(Nav::Down)
            .pre_effects((0..10).map(make_effect))
            .post_effects((0..10).map(make_effect))
            .neg_fields(0..10)
            .next(Label(1))
            .into()],
        def_entries: Default::default(),
        preamble_entry: Label(0),
    };

    lower(&mut result);

    // Should handle all overflows
    for instr in &result.instructions {
        if let InstructionIR::Match(m) = instr {
            assert!(m.pre_effects.len() <= MAX_PRE_EFFECTS);
            assert!(m.post_effects.len() <= MAX_POST_EFFECTS);
            assert!(m.neg_fields.len() <= MAX_NEG_FIELDS);
            assert!(m.successors.len() <= MAX_MATCH_PAYLOAD_SLOTS);
        }
    }
}
