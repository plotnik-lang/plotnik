use std::num::NonZeroU16;

use super::layout::CacheAligned;
use plotnik_bytecode::Nav;
use crate::bytecode::{CallIR, EffectIR, Label, MatchIR, NodeTypeIR, ReturnIR};

#[test]
fn layout_empty() {
    let result = CacheAligned::layout(&[], &[]);

    assert_eq!(result.total_steps, 0);
    assert!(result.label_to_step.is_empty());
}

#[test]
fn layout_single_instruction() {
    let instructions = vec![
        MatchIR::terminal(Label(0))
            .nav(Nav::Down)
            .node_type(NodeTypeIR::Named(NonZeroU16::new(10)))
            .into(),
    ];

    let result = CacheAligned::layout(&instructions, &[Label(0)]);

    assert_eq!(result.label_to_step.get(&Label(0)), Some(&0u16));
    assert_eq!(result.total_steps, 1);
}

#[test]
fn layout_linear_chain() {
    // A -> B -> C -> ACCEPT
    let instructions = vec![
        MatchIR::at(Label(0))
            .nav(Nav::Down)
            .node_type(NodeTypeIR::Named(NonZeroU16::new(10)))
            .next(Label(1))
            .into(),
        MatchIR::at(Label(1))
            .nav(Nav::Next)
            .node_type(NodeTypeIR::Named(NonZeroU16::new(20)))
            .next(Label(2))
            .into(),
        MatchIR::terminal(Label(2)).nav(Nav::Up(1)).into(),
    ];

    let result = CacheAligned::layout(&instructions, &[Label(0)]);

    // Should be contiguous: 0, 1, 2
    assert_eq!(result.label_to_step.get(&Label(0)), Some(&0u16));
    assert_eq!(result.label_to_step.get(&Label(1)), Some(&1u16));
    assert_eq!(result.label_to_step.get(&Label(2)), Some(&2u16));
}

#[test]
fn layout_call_return() {
    // Entry -> Call(target=2) -> Return
    let instructions = vec![
        MatchIR::at(Label(0))
            .nav(Nav::Down)
            .node_type(NodeTypeIR::Named(NonZeroU16::new(10)))
            .next(Label(1))
            .into(),
        CallIR::new(Label(1), Label(2), Label(3))
            .nav(Nav::Down)
            .into(),
        MatchIR::at(Label(2))
            .nav(Nav::Down)
            .node_type(NodeTypeIR::Named(NonZeroU16::new(20)))
            .next(Label(4))
            .into(),
        MatchIR::terminal(Label(3)).nav(Nav::Up(1)).into(),
        ReturnIR::new(Label(4)).into(),
    ];

    let result = CacheAligned::layout(&instructions, &[Label(0)]);

    // All labels should have valid step IDs
    assert!(result.label_to_step.contains_key(&Label(0)));
    assert!(result.label_to_step.contains_key(&Label(1)));
    assert!(result.label_to_step.contains_key(&Label(2)));
    assert!(result.label_to_step.contains_key(&Label(3)));
    assert!(result.label_to_step.contains_key(&Label(4)));
}

#[test]
fn layout_branch() {
    // Entry -> [A, B] -> ACCEPT
    let instructions = vec![
        MatchIR::at(Label(0))
            .next_many(vec![Label(1), Label(2)])
            .into(),
        MatchIR::terminal(Label(1))
            .nav(Nav::Down)
            .node_type(NodeTypeIR::Named(NonZeroU16::new(10)))
            .into(),
        MatchIR::terminal(Label(2))
            .nav(Nav::Down)
            .node_type(NodeTypeIR::Named(NonZeroU16::new(20)))
            .into(),
    ];

    let result = CacheAligned::layout(&instructions, &[Label(0)]);

    // All should have distinct step IDs
    let step0 = result.label_to_step.get(&Label(0)).unwrap();
    let step1 = result.label_to_step.get(&Label(1)).unwrap();
    let step2 = result.label_to_step.get(&Label(2)).unwrap();

    assert_ne!(step0, step1);
    assert_ne!(step1, step2);
    assert_ne!(step0, step2);
}

#[test]
fn layout_match16_cache_alignment() {
    // Match16 (16 bytes, 2 steps) at offset 56 would straddle (56+16=72 > 64)
    // Place 7 Match8s (7 steps = 56 bytes) before Match16
    // Expected: Match16 gets padded to step 8 (offset 64)
    let mut instructions = Vec::new();

    // 7 Match8 instructions in a chain: Label(0) -> Label(1) -> ... -> Label(6) -> Label(7)
    for i in 0..7 {
        instructions.push(
            MatchIR::at(Label(i))
                .nav(Nav::Down)
                .next(Label(i + 1))
                .into(),
        );
    }

    // Match16 at Label(7): needs 2+ successors to become Match16
    instructions.push(
        MatchIR::at(Label(7))
            .nav(Nav::Down)
            .next_many(vec![Label(100), Label(101)])
            .into(),
    );

    let result = CacheAligned::layout(&instructions, &[Label(0)]);

    // Labels 0-6 should be at steps 0-6 (no padding needed)
    for i in 0..7 {
        assert_eq!(
            result.label_to_step.get(&Label(i)),
            Some(&(i as u16)),
            "Label({i}) should be at step {i}"
        );
    }

    // Label(7) would be at step 7 (offset 56) without padding
    // But Match16 at offset 56 straddles (56+16=72 > 64), so it must be padded
    // After padding: step 8 (offset 64)
    let step7 = *result.label_to_step.get(&Label(7)).unwrap();
    assert_eq!(step7, 8, "Match16 should be padded to step 8 (offset 64)");

    // Total steps: 8 (padding at step 7) + 2 (Match16) = 10
    assert_eq!(result.total_steps, 10);
}

#[test]
fn layout_match8_no_padding_needed() {
    // Match8 (8 bytes) never straddles: max offset 56, 56+8=64 <= 64
    // Place 7 Match8s, then another Match8 - should NOT need padding
    let mut instructions = Vec::new();

    for i in 0..8 {
        if i < 7 {
            instructions.push(
                MatchIR::at(Label(i))
                    .nav(Nav::Down)
                    .next(Label(i + 1))
                    .into(),
            );
        } else {
            instructions.push(MatchIR::terminal(Label(i)).nav(Nav::Down).into());
        }
    }

    let result = CacheAligned::layout(&instructions, &[Label(0)]);

    // All 8 Match8s should be contiguous: steps 0-7
    for i in 0..8 {
        assert_eq!(
            result.label_to_step.get(&Label(i)),
            Some(&(i as u16)),
            "Label({i}) should be at step {i} (no padding)"
        );
    }

    // Total steps: 8 (no padding)
    assert_eq!(result.total_steps, 8);
}

#[test]
fn layout_match32_cache_alignment() {
    // Match32 (32 bytes, 4 steps) at offset 40 would straddle (40+32=72 > 64)
    // Place 5 Match8s (5 steps = 40 bytes) before Match32
    // Expected: Match32 gets padded to step 8 (offset 64)
    let mut instructions: Vec<crate::bytecode::InstructionIR> = Vec::new();

    // 5 Match8 instructions: Label(0) -> ... -> Label(4) -> Label(5)
    for i in 0..5 {
        instructions.push(
            MatchIR::at(Label(i))
                .nav(Nav::Down)
                .next(Label(i + 1))
                .into(),
        );
    }

    // Match32 at Label(5): needs enough payload to become Match32 (9-12 slots)
    // 3 pre + 3 post + 4 successors = 10 slots -> Match32
    instructions.push(
        MatchIR::at(Label(5))
            .nav(Nav::Down)
            .pre_effect(EffectIR::start_obj())
            .pre_effect(EffectIR::start_obj())
            .pre_effect(EffectIR::start_obj())
            .post_effect(EffectIR::end_obj())
            .post_effect(EffectIR::end_obj())
            .post_effect(EffectIR::end_obj())
            .next_many(vec![Label(100), Label(101), Label(102), Label(103)])
            .into(),
    );

    // Verify it's Match32 (32 bytes)
    assert_eq!(instructions.last().unwrap().size(), 32);

    let result = CacheAligned::layout(&instructions, &[Label(0)]);

    // Labels 0-4 at steps 0-4
    for i in 0..5 {
        assert_eq!(result.label_to_step.get(&Label(i)), Some(&(i as u16)));
    }

    // Label(5) would be at step 5 (offset 40) without padding
    // Match32 at offset 40 straddles (40+32=72 > 64), so padded to step 8
    let step5 = *result.label_to_step.get(&Label(5)).unwrap();
    assert_eq!(step5, 8, "Match32 should be padded to step 8 (offset 64)");

    // Total steps: 8 (3 padding steps at 5,6,7) + 4 (Match32) = 12
    assert_eq!(result.total_steps, 12);
}
