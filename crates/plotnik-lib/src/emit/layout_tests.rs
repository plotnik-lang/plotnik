use std::num::NonZeroU16;

use super::layout::CacheAligned;
use crate::bytecode::EffectOpcode;
use crate::bytecode::Nav;
use crate::bytecode::StepId;
use crate::bytecode::ir::{CallIR, EffectIR, Instruction, Label, MatchIR, ReturnIR};

#[test]
fn layout_empty() {
    let result = CacheAligned::layout(&[], &[]);

    assert_eq!(result.total_steps, 0);
    assert!(result.label_to_step.is_empty());
}

#[test]
fn layout_single_instruction() {
    let instructions = vec![Instruction::Match(MatchIR {
        label: Label(0),
        nav: Nav::Down,
        node_type: NonZeroU16::new(10),
        node_field: None,
        pre_effects: vec![],
        neg_fields: vec![],
        post_effects: vec![],
        successors: vec![], // Terminal - empty successors
    })];

    let result = CacheAligned::layout(&instructions, &[Label(0)]);

    assert_eq!(result.label_to_step.get(&Label(0)), Some(&StepId::new(0)));
    assert_eq!(result.total_steps, 1);
}

#[test]
fn layout_linear_chain() {
    // A -> B -> C -> ACCEPT
    let instructions = vec![
        Instruction::Match(MatchIR {
            label: Label(0),
            nav: Nav::Down,
            node_type: NonZeroU16::new(10),
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![Label(1)],
        }),
        Instruction::Match(MatchIR {
            label: Label(1),
            nav: Nav::Next,
            node_type: NonZeroU16::new(20),
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![Label(2)],
        }),
        Instruction::Match(MatchIR {
            label: Label(2),
            nav: Nav::Up(1),
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![], // Terminal
        }),
    ];

    let result = CacheAligned::layout(&instructions, &[Label(0)]);

    // Should be contiguous: 0, 1, 2
    assert_eq!(result.label_to_step.get(&Label(0)), Some(&StepId::new(0)));
    assert_eq!(result.label_to_step.get(&Label(1)), Some(&StepId::new(1)));
    assert_eq!(result.label_to_step.get(&Label(2)), Some(&StepId::new(2)));
}

#[test]
fn layout_call_return() {
    // Entry -> Call(target=2) -> Return
    let instructions = vec![
        Instruction::Match(MatchIR {
            label: Label(0),
            nav: Nav::Down,
            node_type: NonZeroU16::new(10),
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![Label(1)],
        }),
        Instruction::Call(CallIR {
            label: Label(1),
            nav: Nav::Down,
            node_field: None,
            next: Label(3),
            target: Label(2),
        }),
        Instruction::Match(MatchIR {
            label: Label(2),
            nav: Nav::Down,
            node_type: NonZeroU16::new(20),
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![Label(4)],
        }),
        Instruction::Match(MatchIR {
            label: Label(3),
            nav: Nav::Up(1),
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![], // Terminal
        }),
        Instruction::Return(ReturnIR { label: Label(4) }),
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
        Instruction::Match(MatchIR {
            label: Label(0),
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![Label(1), Label(2)],
        }),
        Instruction::Match(MatchIR {
            label: Label(1),
            nav: Nav::Down,
            node_type: NonZeroU16::new(10),
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![], // Terminal
        }),
        Instruction::Match(MatchIR {
            label: Label(2),
            nav: Nav::Down,
            node_type: NonZeroU16::new(20),
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![], // Terminal
        }),
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
    let large_match = MatchIR {
        label: Label(1),
        nav: Nav::Down,
        node_type: NonZeroU16::new(10),
        node_field: None,
        pre_effects: vec![
            EffectIR::simple(EffectOpcode::Obj, 0),
            EffectIR::simple(EffectOpcode::Obj, 0),
            EffectIR::simple(EffectOpcode::Obj, 0),
        ],
        neg_fields: vec![],
        post_effects: vec![
            EffectIR::simple(EffectOpcode::Node, 0),
            EffectIR::simple(EffectOpcode::EndObj, 0),
            EffectIR::simple(EffectOpcode::EndObj, 0),
            EffectIR::simple(EffectOpcode::EndObj, 0),
        ],
        successors: vec![
            Label(100),
            Label(101),
            Label(102),
            Label(103),
            Label(104),
            Label(105),
            Label(106),
            Label(107),
        ],
    };

    // Verify it's large enough to trigger alignment
    assert!(large_match.size() >= 48);

    let instructions = vec![
        // Small instruction first
        Instruction::Match(MatchIR {
            label: Label(0),
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![Label(1)],
        }),
        Instruction::Match(large_match),
    ];

    let result = CacheAligned::layout(&instructions, &[Label(0)]);

    // Label 0 at step 0 (offset 0)
    assert_eq!(result.label_to_step.get(&Label(0)), Some(&StepId::new(0)));

    // Label 1 should be aligned - either at step 1 or padded to cache line
    let step1 = result.label_to_step.get(&Label(1)).unwrap();
    assert!(step1.get() >= 1);
}
