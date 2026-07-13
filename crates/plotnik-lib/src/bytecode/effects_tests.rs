use super::*;
use crate::bytecode::effects::EFFECT_PAYLOAD_BITS;

#[test]
fn roundtrip_with_payload() {
    let op = Effect::new(EffectKind::Set, 42);
    let bytes = op.to_bytes();
    let decoded = Effect::from_bytes(bytes);
    assert_eq!(decoded.kind, EffectKind::Set);
    assert_eq!(decoded.payload, 42);
}

#[test]
fn roundtrip_no_payload() {
    let op = Effect::new(EffectKind::Node, 0);
    let bytes = op.to_bytes();
    let decoded = Effect::from_bytes(bytes);
    assert_eq!(decoded.kind, EffectKind::Node);
    assert_eq!(decoded.payload, 0);
}

#[test]
fn max_payload() {
    let op = Effect::new(EffectKind::EnumOpen, 1023);
    let bytes = op.to_bytes();
    let decoded = Effect::from_bytes(bytes);
    assert_eq!(decoded.payload, 1023);
}

#[test]
fn span_effects_roundtrip() {
    for kind in [
        EffectKind::SpanStartAt,
        EffectKind::SpanStart,
        EffectKind::SpanEnd,
    ] {
        let op = Effect::new(kind, 7);
        let decoded = Effect::from_bytes(op.to_bytes());
        assert_eq!(decoded.kind, kind);
        assert_eq!(decoded.payload, 7);
    }
}

#[test]
fn opcode_after_scalar_effects_is_rejected() {
    let invalid = EffectKind::BoolValue as u16 + 1;
    let raw = (invalid << EFFECT_PAYLOAD_BITS).to_le_bytes();
    assert!(Effect::try_from_bytes(raw).is_none());
}

#[test]
fn scalar_effect_metadata_preserves_motion_and_frame_boundaries() {
    assert_eq!(
        EffectKind::ScalarOpen.frame_action(),
        Some(FrameAction::Open(ValueFrameKind::Scalar))
    );
    assert_eq!(
        EffectKind::StrClose.frame_action(),
        Some(FrameAction::Close(ValueFrameKind::Scalar))
    );
    assert_eq!(
        EffectKind::BoolClose.frame_action(),
        Some(FrameAction::Close(ValueFrameKind::Scalar))
    );
    assert!(EffectKind::ScalarMark.reads_cursor());
    assert!(EffectKind::ScalarOpen.is_motion_barrier());
    assert!(EffectKind::StrClose.is_motion_barrier());
    assert!(EffectKind::BoolClose.is_motion_barrier());
    assert!(EffectKind::NodeStr.reads_cursor());
    assert!(EffectKind::NodeBool.reads_cursor());
    assert!(!EffectKind::BoolValue.reads_cursor());
    assert!(EffectKind::BoolValue.accepts_payload(0, 0, 0));
    assert!(EffectKind::BoolValue.accepts_payload(1, 0, 0));
    assert!(!EffectKind::BoolValue.accepts_payload(2, 0, 0));
}

#[test]
#[should_panic(expected = "invalid effect opcode")]
fn invalid_opcode_panics() {
    let bytes = [0xFF, 0xFF]; // opcode would be 63, which is invalid
    Effect::from_bytes(bytes);
}
