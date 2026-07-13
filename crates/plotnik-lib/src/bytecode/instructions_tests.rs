//! Tests for bytecode instructions.

use std::num::NonZeroU16;

use proptest::prelude::*;

use crate::core::{NodeFieldId, NodeKindId};

use super::effects::{Effect, EffectKind};
use super::instructions::{
    Call, EncodeError, Match, MatchInstr, MatchPredicate, Opcode, Return, RoutedCall, SplitCall,
    SplitCallReturns, SuccessorAddr, align_to_section, select_match_opcode,
};
use super::node_kind_constraint::NodeKindConstraint;
use plotnik_rt::Nav;

#[test]
fn from_u8_decodes_known_and_rejects_unknown() {
    let known = [
        (0x0u8, Opcode::Match8),
        (0x1, Opcode::Match16),
        (0x2, Opcode::Match24),
        (0x3, Opcode::Match32),
        (0x4, Opcode::Match48),
        (0x5, Opcode::Match64),
        (0x6, Opcode::Call),
        (0x7, Opcode::Return),
        (0x8, Opcode::SplitCall),
        (0x9, Opcode::RoutedCall),
    ];

    for (nibble, expected) in known {
        assert_eq!(Opcode::from_u8(nibble), Some(expected));
    }
    for nibble in 0xAu8..=0xF {
        assert_eq!(Opcode::from_u8(nibble), None, "nibble {nibble:#x}");
    }
}

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
    assert_eq!(Opcode::SplitCall.size(), 8);
    assert_eq!(Opcode::RoutedCall.size(), 8);
}

#[test]
fn opcode_word_counts() {
    assert_eq!(Opcode::Match8.word_count(), 1);
    assert_eq!(Opcode::Match16.word_count(), 2);
    assert_eq!(Opcode::Match32.word_count(), 4);
    assert_eq!(Opcode::Match64.word_count(), 8);
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
        NonZeroU16::new(42).map(NodeFieldId::from),
        SuccessorAddr::try_from(100).expect("successor address must be non-zero"),
        SuccessorAddr::try_from(500).expect("successor address must be non-zero"),
    );

    let bytes = c.to_bytes();
    let decoded = Call::from_bytes(bytes);
    assert_eq!(decoded, c);
}

#[test]
fn return_roundtrip() {
    for r in [
        Return::matched(),
        Return::routed_matched(),
        Return::routed_empty(),
    ] {
        let bytes = r.to_bytes();
        let decoded = Return::from_bytes(bytes);
        assert_eq!(decoded, r);
    }
}

#[test]
fn split_call_roundtrip() {
    let call = SplitCall::new(
        Nav::Next,
        SplitCallReturns {
            matched: SuccessorAddr::try_from(100).expect("successor address must be non-zero"),
            empty: SuccessorAddr::try_from(200).expect("successor address must be non-zero"),
        },
        SuccessorAddr::try_from(500).expect("successor address must be non-zero"),
    );

    assert_eq!(SplitCall::from_bytes(call.to_bytes()), call);
}

#[test]
fn routed_call_roundtrip() {
    let call = RoutedCall::new(
        Nav::Next,
        SuccessorAddr::try_from(100).expect("successor address must be non-zero"),
        SuccessorAddr::try_from(500).expect("successor address must be non-zero"),
    );

    assert_eq!(RoutedCall::from_bytes(call.to_bytes()), call);
}

#[test]
fn encode_rejects_effect_payload_overflow() {
    let instr = MatchInstr {
        effects: vec![Effect::new(EffectKind::RecordSet, 0x400)],
        successors: vec![SuccessorAddr::try_from(1).expect("successor address must be non-zero")],
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
        successors: (1u16..=32)
            .map(|n| SuccessorAddr::try_from(n).unwrap())
            .collect(),
        ..Default::default()
    };

    assert_eq!(instr.encode(), Err(EncodeError::TooManySuccessors(32)));
}

#[test]
fn encode_rejects_oversized_payload() {
    // 29 successors is under the 31 cap, but 29 slots exceeds Match64's 28.
    let instr = MatchInstr {
        successors: (1u16..=29)
            .map(|n| SuccessorAddr::try_from(n).unwrap())
            .collect(),
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
        (1u8..=Nav::MAX_UP_LEVEL).prop_map(Nav::Up),
        (1u8..=Nav::MAX_UP_LEVEL).prop_map(Nav::UpSkipTrivia),
        (1u8..=Nav::MAX_UP_LEVEL).prop_map(Nav::UpSkipExtras),
        (1u8..=Nav::MAX_UP_LEVEL).prop_map(Nav::UpExact),
    ]
}

fn arb_node_type() -> impl Strategy<Value = NodeKindConstraint> {
    prop_oneof![
        Just(NodeKindConstraint::Any),
        Just(NodeKindConstraint::Named(None)),
        (1u16..=u16::MAX)
            .prop_map(|n| NodeKindConstraint::Named(NonZeroU16::new(n).map(NodeKindId::from))),
        Just(NodeKindConstraint::Anonymous(None)),
        (1u16..=u16::MAX)
            .prop_map(|n| NodeKindConstraint::Anonymous(NonZeroU16::new(n).map(NodeKindId::from))),
    ]
}

fn arb_effect() -> impl Strategy<Value = Effect> {
    let kind = prop::sample::select(vec![
        EffectKind::Node,
        EffectKind::ListOpen,
        EffectKind::ArrayPush,
        EffectKind::ListClose,
        EffectKind::RecordOpen,
        EffectKind::RecordClose,
        EffectKind::RecordSet,
        EffectKind::VariantOpen,
        EffectKind::VariantClose,
        EffectKind::Absent,
        EffectKind::SuppressBegin,
        EffectKind::SuppressEnd,
        EffectKind::SpanStartAt,
        EffectKind::SpanStart,
        EffectKind::SpanEnd,
    ]);
    (kind, 0usize..=0x3FF).prop_map(|(kind, payload)| Effect::new(kind, payload))
}

fn arb_predicate() -> impl Strategy<Value = MatchPredicate> {
    (0u8..=6, any::<bool>(), any::<u16>()).prop_map(|(op, is_regex, value_ref)| MatchPredicate {
        op,
        is_regex,
        value_ref,
    })
}

fn arb_match_instr() -> impl Strategy<Value = MatchInstr> {
    // Per-field caps keep the worst case (15 + 7 + 2 predicate + 4) at exactly
    // Match64's 28-slot ceiling, so every generated instruction encodes.
    (
        arb_nav(),
        arb_node_type(),
        prop::option::of((1u16..=u16::MAX).prop_map(|n| NodeFieldId::try_from(n).unwrap())),
        any::<bool>(),
        prop::collection::vec(arb_effect(), 0..=15),
        prop::collection::vec(
            (1u16..=u16::MAX).prop_map(|n| NodeFieldId::try_from(n).unwrap()),
            0..=7,
        ),
        prop::option::of(arb_predicate()),
        prop::collection::vec(
            (1u16..=u16::MAX).prop_map(|n| SuccessorAddr::try_from(n).unwrap()),
            0..=4,
        ),
    )
        .prop_map(
            |(nav, node_kind, node_field, missing, effects, neg_fields, predicate, successors)| {
                MatchInstr {
                    nav,
                    node_kind,
                    node_field,
                    missing,
                    effects,
                    neg_fields,
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
