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
fn reserved_span_extension_kind_is_rejected() {
    let raw = (15u16 << EFFECT_PAYLOAD_BITS).to_le_bytes();
    assert!(Effect::try_from_bytes(raw).is_none());
}

#[test]
#[should_panic(expected = "invalid effect opcode")]
fn invalid_opcode_panics() {
    let bytes = [0xFF, 0xFF]; // opcode would be 63, which is invalid
    Effect::from_bytes(bytes);
}
