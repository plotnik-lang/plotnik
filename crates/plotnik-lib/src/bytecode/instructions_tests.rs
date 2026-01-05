//! Tests for bytecode instructions.

use std::num::NonZeroU16;

use super::effects::{EffectOp, EffectOpcode};
use super::instructions::{
    Call, Match, MatchView, Opcode, Return, StepId, align_to_section, select_match_opcode,
};
use super::nav::Nav;

#[test]
fn opcode_sizes() {
    assert_eq!(Opcode::Match8.size(), 8);
    assert_eq!(Opcode::Match16.size(), 16);
    assert_eq!(Opcode::Match24.size(), 24);
    assert_eq!(Opcode::Match32.size(), 32);
    assert_eq!(Opcode::Match48.size(), 48);
    assert_eq!(Opcode::Match64.size(), 64);
    assert_eq!(Opcode::Call.size(), 8);
    assert_eq!(Opcode::Return.size(), 8);
}

#[test]
fn opcode_step_counts() {
    assert_eq!(Opcode::Match8.step_count(), 1);
    assert_eq!(Opcode::Match16.step_count(), 2);
    assert_eq!(Opcode::Match32.step_count(), 4);
    assert_eq!(Opcode::Match64.step_count(), 8);
}

#[test]
fn opcode_payload_slots() {
    assert_eq!(Opcode::Match8.payload_slots(), 0);
    assert_eq!(Opcode::Match16.payload_slots(), 4);
    assert_eq!(Opcode::Match24.payload_slots(), 8);
    assert_eq!(Opcode::Match32.payload_slots(), 12);
    assert_eq!(Opcode::Match48.payload_slots(), 20);
    assert_eq!(Opcode::Match64.payload_slots(), 28);
}

#[test]
fn select_match_opcode_picks_smallest() {
    assert_eq!(select_match_opcode(0), Some(Opcode::Match8));
    assert_eq!(select_match_opcode(1), Some(Opcode::Match16));
    assert_eq!(select_match_opcode(4), Some(Opcode::Match16));
    assert_eq!(select_match_opcode(5), Some(Opcode::Match24));
    assert_eq!(select_match_opcode(12), Some(Opcode::Match32));
    assert_eq!(select_match_opcode(20), Some(Opcode::Match48));
    assert_eq!(select_match_opcode(28), Some(Opcode::Match64));
    assert_eq!(select_match_opcode(29), None);
}

#[test]
fn align_to_section_works() {
    assert_eq!(align_to_section(0), 0);
    assert_eq!(align_to_section(1), 64);
    assert_eq!(align_to_section(64), 64);
    assert_eq!(align_to_section(65), 128);
    assert_eq!(align_to_section(100), 128);
}

#[test]
fn match8_roundtrip() {
    let m = Match {
        segment: 0,
        nav: Nav::Down,
        node_type: NonZeroU16::new(42),
        node_field: NonZeroU16::new(7),
        pre_effects: vec![],
        neg_fields: vec![],
        post_effects: vec![],
        successors: vec![StepId::new(10)],
    };

    let bytes = m.to_bytes().unwrap();
    assert_eq!(bytes.len(), 8);

    let decoded = Match::from_bytes(&bytes);
    assert_eq!(decoded, m);
}

#[test]
fn match8_terminal_roundtrip() {
    let m = Match {
        segment: 0,
        nav: Nav::Stay,
        node_type: None,
        node_field: None,
        pre_effects: vec![],
        neg_fields: vec![],
        post_effects: vec![],
        successors: vec![],
    };

    let bytes = m.to_bytes().unwrap();
    assert_eq!(bytes.len(), 8);

    let decoded = Match::from_bytes(&bytes);
    assert_eq!(decoded, m);
    assert!(decoded.is_terminal());
    assert!(decoded.is_epsilon());
}

#[test]
fn match_extended_roundtrip() {
    let m = Match {
        segment: 0,
        nav: Nav::Next,
        node_type: NonZeroU16::new(100),
        node_field: None,
        pre_effects: vec![EffectOp {
            opcode: EffectOpcode::Obj,
            payload: 0,
        }],
        neg_fields: vec![5, 6],
        post_effects: vec![
            EffectOp {
                opcode: EffectOpcode::Node,
                payload: 0,
            },
            EffectOp {
                opcode: EffectOpcode::Set,
                payload: 42,
            },
        ],
        successors: vec![StepId::new(20), StepId::new(30)],
    };

    let bytes = m.to_bytes().unwrap();
    // 1 pre + 2 neg + 2 post + 2 succ = 7 slots → Match24 (8 slots capacity)
    assert_eq!(bytes.len(), 24);

    let decoded = Match::from_bytes(&bytes);
    assert_eq!(decoded, m);
}

#[test]
fn call_roundtrip() {
    let c = Call {
        segment: 0,
        nav: Nav::Down,
        node_field: NonZeroU16::new(42),
        next: StepId::new(100),
        target: StepId::new(500),
    };

    let bytes = c.to_bytes();
    let decoded = Call::from_bytes(bytes);
    assert_eq!(decoded, c);
}

#[test]
fn return_roundtrip() {
    let r = Return { segment: 0 };

    let bytes = r.to_bytes();
    let decoded = Return::from_bytes(bytes);
    assert_eq!(decoded, r);
}

#[test]
fn match_view_match8() {
    let m = Match {
        segment: 0,
        nav: Nav::Down,
        node_type: NonZeroU16::new(42),
        node_field: NonZeroU16::new(7),
        pre_effects: vec![],
        neg_fields: vec![],
        post_effects: vec![],
        successors: vec![StepId::new(10)],
    };

    let bytes = m.to_bytes().unwrap();
    let view = MatchView::from_bytes(&bytes);

    assert_eq!(view.nav, Nav::Down);
    assert_eq!(view.node_type, NonZeroU16::new(42));
    assert_eq!(view.node_field, NonZeroU16::new(7));
    assert!(!view.is_terminal());
    assert!(!view.is_epsilon());
    assert_eq!(view.succ_count(), 1);
    assert_eq!(view.successor(0), StepId::new(10));
    assert_eq!(view.pre_effects().count(), 0);
    assert_eq!(view.neg_fields().count(), 0);
    assert_eq!(view.post_effects().count(), 0);
}

#[test]
fn match_view_terminal() {
    let m = Match {
        segment: 0,
        nav: Nav::Stay,
        node_type: None,
        node_field: None,
        pre_effects: vec![],
        neg_fields: vec![],
        post_effects: vec![],
        successors: vec![],
    };

    let bytes = m.to_bytes().unwrap();
    let view = MatchView::from_bytes(&bytes);

    assert!(view.is_terminal());
    assert!(view.is_epsilon());
    assert_eq!(view.succ_count(), 0);
}

#[test]
fn match_view_extended() {
    let m = Match {
        segment: 0,
        nav: Nav::Next,
        node_type: NonZeroU16::new(100),
        node_field: None,
        pre_effects: vec![EffectOp {
            opcode: EffectOpcode::Obj,
            payload: 0,
        }],
        neg_fields: vec![5, 6],
        post_effects: vec![
            EffectOp {
                opcode: EffectOpcode::Node,
                payload: 0,
            },
            EffectOp {
                opcode: EffectOpcode::Set,
                payload: 42,
            },
        ],
        successors: vec![StepId::new(20), StepId::new(30)],
    };

    let bytes = m.to_bytes().unwrap();
    let view = MatchView::from_bytes(&bytes);

    assert_eq!(view.nav, Nav::Next);
    assert_eq!(view.node_type, NonZeroU16::new(100));
    assert!(!view.is_terminal());

    // Check pre_effects
    let pre: Vec<_> = view.pre_effects().collect();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0].opcode, EffectOpcode::Obj);

    // Check neg_fields
    let neg: Vec<_> = view.neg_fields().collect();
    assert_eq!(neg, vec![5, 6]);

    // Check post_effects
    let post: Vec<_> = view.post_effects().collect();
    assert_eq!(post.len(), 2);
    assert_eq!(post[0].opcode, EffectOpcode::Node);
    assert_eq!(post[1].opcode, EffectOpcode::Set);
    assert_eq!(post[1].payload, 42);

    // Check successors
    assert_eq!(view.succ_count(), 2);
    assert_eq!(view.successor(0), StepId::new(20));
    assert_eq!(view.successor(1), StepId::new(30));
    let succs: Vec<_> = view.successors().collect();
    assert_eq!(succs, vec![StepId::new(20), StepId::new(30)]);
}
