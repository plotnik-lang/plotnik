//! Tests for bytecode instructions.

use std::collections::BTreeMap;
use std::num::NonZeroU16;

use super::effects::EffectOpcode;
use super::instructions::{
    Call, Match, Opcode, Return, StepId, align_to_section, select_match_opcode,
};
use super::ir::{EffectIR, Label, MatchIR};
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
fn call_roundtrip() {
    let c = Call::new(
        Nav::Down,
        NonZeroU16::new(42),
        StepId::new(100),
        StepId::new(500),
    );

    let bytes = c.to_bytes();
    let decoded = Call::from_bytes(bytes);
    assert_eq!(decoded, c);
}

#[test]
fn return_roundtrip() {
    let r = Return::new();

    let bytes = r.to_bytes();
    let decoded = Return::from_bytes(bytes);
    assert_eq!(decoded, r);
}

fn label_map(pairs: &[(u32, u16)]) -> BTreeMap<Label, u16> {
    pairs.iter().map(|&(l, s)| (Label(l), s)).collect()
}

#[test]
fn match_basic() {
    let map = label_map(&[(0, 1), (1, 10)]);

    let bytes = MatchIR::at(Label(0))
        .nav(Nav::Down)
        .node_type(NonZeroU16::new(42))
        .node_field(NonZeroU16::new(7))
        .next(Label(1))
        .resolve(&map, |_, _| None, |_| None);

    assert_eq!(bytes.len(), 8);

    let m = Match::from_bytes(&bytes);
    assert_eq!(m.nav, Nav::Down);
    assert_eq!(m.node_type, NonZeroU16::new(42));
    assert_eq!(m.node_field, NonZeroU16::new(7));
    assert!(!m.is_terminal());
    assert!(!m.is_epsilon());
    assert_eq!(m.succ_count(), 1);
    assert_eq!(m.successor(0), StepId::new(10));
    assert_eq!(m.pre_effects().count(), 0);
    assert_eq!(m.neg_fields().count(), 0);
    assert_eq!(m.post_effects().count(), 0);
}

#[test]
fn match_terminal() {
    let map = label_map(&[(0, 1)]);

    let bytes = MatchIR::terminal(Label(0)).resolve(&map, |_, _| None, |_| None);

    assert_eq!(bytes.len(), 8);

    let m = Match::from_bytes(&bytes);
    assert!(m.is_terminal());
    assert!(m.is_epsilon());
    assert_eq!(m.succ_count(), 0);
}

#[test]
fn match_extended() {
    let map = label_map(&[(0, 1), (1, 20), (2, 30)]);

    let bytes = MatchIR::at(Label(0))
        .nav(Nav::Next)
        .node_type(NonZeroU16::new(100))
        .pre_effect(EffectIR::start_obj())
        .neg_field(5)
        .neg_field(6)
        .post_effect(EffectIR::node())
        .post_effect(EffectIR::with_member(
            EffectOpcode::Set,
            super::ir::MemberRef::absolute(42),
        ))
        .next_many(vec![Label(1), Label(2)])
        .resolve(&map, |_, _| None, |_| None);

    // 1 pre + 2 neg + 2 post + 2 succ = 7 slots → Match24 (8 slots capacity)
    assert_eq!(bytes.len(), 24);

    let m = Match::from_bytes(&bytes);
    assert_eq!(m.nav, Nav::Next);
    assert_eq!(m.node_type, NonZeroU16::new(100));
    assert!(!m.is_terminal());

    let pre: Vec<_> = m.pre_effects().collect();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0].opcode, EffectOpcode::Obj);

    let neg: Vec<_> = m.neg_fields().collect();
    assert_eq!(neg, vec![5, 6]);

    let post: Vec<_> = m.post_effects().collect();
    assert_eq!(post.len(), 2);
    assert_eq!(post[0].opcode, EffectOpcode::Node);
    assert_eq!(post[1].opcode, EffectOpcode::Set);
    assert_eq!(post[1].payload, 42);

    assert_eq!(m.succ_count(), 2);
    assert_eq!(m.successor(0), StepId::new(20));
    assert_eq!(m.successor(1), StepId::new(30));
    let succs: Vec<_> = m.successors().collect();
    assert_eq!(succs, vec![StepId::new(20), StepId::new(30)]);
}
