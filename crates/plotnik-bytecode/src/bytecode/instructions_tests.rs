//! Tests for bytecode instructions.

use std::num::NonZeroU16;

use proptest::prelude::*;

use super::effects::{EffectOp, EffectOpcode};
use super::instructions::{
    Call, EncodeError, Match, MatchInstr, MatchPredicate, Opcode, Return, StepId, Trampoline,
    align_to_section, select_match_opcode,
};
use super::nav::Nav;
use super::node_type_ir::NodeTypeIR;

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

#[test]
fn trampoline_roundtrip() {
    let t = Trampoline::new(StepId::new(7));

    let bytes = t.to_bytes();
    let decoded = Trampoline::from_bytes(bytes);
    assert_eq!(decoded, t);
}

#[test]
fn encode_rejects_effect_payload_overflow() {
    let instr = MatchInstr {
        post_effects: vec![EffectOp::new(EffectOpcode::Set, 0x400)],
        successors: vec![StepId::new(1)],
        ..Default::default()
    };

    assert_eq!(
        instr.encode(),
        Err(EncodeError::EffectPayloadOverflow(0x400))
    );
}

#[test]
fn encode_rejects_too_many_successors() {
    let instr = MatchInstr {
        successors: (1u16..=32).map(StepId::new).collect(),
        ..Default::default()
    };

    assert_eq!(instr.encode(), Err(EncodeError::TooManySuccessors(32)));
}

#[test]
fn encode_rejects_oversized_payload() {
    // 29 successors is under the 31 cap, but 29 slots exceeds Match64's 28.
    let instr = MatchInstr {
        successors: (1u16..=29).map(StepId::new).collect(),
        ..Default::default()
    };

    assert_eq!(instr.encode(), Err(EncodeError::PayloadTooLarge(29)));
}

fn arb_nav() -> impl Strategy<Value = Nav> {
    prop_oneof![
        Just(Nav::Epsilon),
        Just(Nav::Stay),
        Just(Nav::StayExact),
        Just(Nav::Next),
        Just(Nav::NextSkip),
        Just(Nav::NextSkipExtras),
        Just(Nav::NextExact),
        Just(Nav::Down),
        Just(Nav::DownSkip),
        Just(Nav::DownSkipExtras),
        Just(Nav::DownExact),
        (1u8..=63).prop_map(Nav::Up),
        (1u8..=63).prop_map(Nav::UpSkipTrivia),
        (1u8..=53).prop_map(Nav::UpSkipExtras),
        (1u8..=63).prop_map(Nav::UpExact),
    ]
}

fn arb_node_type() -> impl Strategy<Value = NodeTypeIR> {
    prop_oneof![
        Just(NodeTypeIR::Any),
        Just(NodeTypeIR::Named(None)),
        (1u16..=u16::MAX).prop_map(|n| NodeTypeIR::Named(NonZeroU16::new(n))),
        Just(NodeTypeIR::Anonymous(None)),
        (1u16..=u16::MAX).prop_map(|n| NodeTypeIR::Anonymous(NonZeroU16::new(n))),
    ]
}

fn arb_effect() -> impl Strategy<Value = EffectOp> {
    let opcode = prop::sample::select(vec![
        EffectOpcode::Node,
        EffectOpcode::Arr,
        EffectOpcode::Push,
        EffectOpcode::EndArr,
        EffectOpcode::Obj,
        EffectOpcode::EndObj,
        EffectOpcode::Set,
        EffectOpcode::Enum,
        EffectOpcode::EndEnum,
        EffectOpcode::Text,
        EffectOpcode::Clear,
        EffectOpcode::Null,
        EffectOpcode::SuppressBegin,
        EffectOpcode::SuppressEnd,
    ]);
    (opcode, 0usize..=0x3FF).prop_map(|(opcode, payload)| EffectOp::new(opcode, payload))
}

fn arb_predicate() -> impl Strategy<Value = MatchPredicate> {
    (0u8..=6, any::<bool>(), any::<u16>()).prop_map(|(op, is_regex, value_ref)| MatchPredicate {
        op,
        is_regex,
        value_ref,
    })
}

fn arb_match_instr() -> impl Strategy<Value = MatchInstr> {
    // Per-field caps keep the worst case (7+7+7 + 2 predicate + 5) at exactly
    // Match64's 28-slot ceiling, so every generated instruction encodes.
    (
        arb_nav(),
        arb_node_type(),
        prop::option::of((1u16..=u16::MAX).prop_map(|n| NonZeroU16::new(n).unwrap())),
        prop::collection::vec(arb_effect(), 0..=7),
        prop::collection::vec(any::<u16>(), 0..=7),
        prop::collection::vec(arb_effect(), 0..=7),
        prop::option::of(arb_predicate()),
        prop::collection::vec((1u16..=u16::MAX).prop_map(StepId::new), 0..=5),
    )
        .prop_map(
            |(
                nav,
                node_type,
                node_field,
                pre_effects,
                neg_fields,
                post_effects,
                predicate,
                successors,
            )| {
                MatchInstr {
                    nav,
                    node_type,
                    node_field,
                    pre_effects,
                    neg_fields,
                    post_effects,
                    predicate,
                    successors,
                }
            },
        )
}

proptest! {
    /// Encoding then decoding any in-bounds Match yields an identical instruction.
    #[test]
    fn match_instr_roundtrip(instr in arb_match_instr()) {
        let bytes = instr.encode().expect("generated instruction is within bounds");
        let decoded = Match::from_bytes(&bytes).to_instr();
        prop_assert_eq!(decoded, instr);
    }
}
