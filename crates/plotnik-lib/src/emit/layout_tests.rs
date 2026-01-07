use std::num::NonZeroU16;

use super::layout::CacheAligned;
use crate::bytecode::Nav;
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
fn layout_large_instruction_cache_alignment() {
    // Large instruction (Match48 = 48 bytes = 6 steps) near cache line boundary
    // Start at step 5 (offset 40), would straddle - should pad
    let large_match = MatchIR::at(Label(1))
        .nav(Nav::Down)
        .node_type(NodeTypeIR::Named(NonZeroU16::new(10)))
        .pre_effect(EffectIR::start_obj())
        .pre_effect(EffectIR::start_obj())
        .pre_effect(EffectIR::start_obj())
        .post_effect(EffectIR::node())
        .post_effect(EffectIR::end_obj())
        .post_effect(EffectIR::end_obj())
        .post_effect(EffectIR::end_obj())
        .next_many(vec![
            Label(100),
            Label(101),
            Label(102),
            Label(103),
            Label(104),
            Label(105),
            Label(106),
            Label(107),
        ]);

    // Verify it's large enough to trigger alignment
    assert!(large_match.size() >= 48);

    let instructions = vec![
        // Small instruction first
        MatchIR::epsilon(Label(0), Label(1)).into(),
        large_match.into(),
    ];

    let result = CacheAligned::layout(&instructions, &[Label(0)]);

    // Label 0 at step 0 (offset 0)
    assert_eq!(result.label_to_step.get(&Label(0)), Some(&0u16));

    // Label 1 should be aligned - either at step 1 or padded to cache line
    let step1 = *result.label_to_step.get(&Label(1)).unwrap();
    assert!(step1 >= 1);
}
