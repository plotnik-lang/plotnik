use super::*;

#[test]
fn roundtrip_with_payload() {
    let op = EffectOp {
        opcode: EffectOpcode::Set,
        payload: 42,
    };
    let bytes = op.to_bytes();
    let decoded = EffectOp::from_bytes(bytes);
    assert_eq!(decoded.opcode, EffectOpcode::Set);
    assert_eq!(decoded.payload, 42);
}

#[test]
fn roundtrip_no_payload() {
    let op = EffectOp {
        opcode: EffectOpcode::Node,
        payload: 0,
    };
    let bytes = op.to_bytes();
    let decoded = EffectOp::from_bytes(bytes);
    assert_eq!(decoded.opcode, EffectOpcode::Node);
    assert_eq!(decoded.payload, 0);
}

#[test]
fn max_payload() {
    let op = EffectOp {
        opcode: EffectOpcode::Enum,
        payload: 1023,
    };
    let bytes = op.to_bytes();
    let decoded = EffectOp::from_bytes(bytes);
    assert_eq!(decoded.payload, 1023);
}

#[test]
#[should_panic(expected = "invalid effect opcode")]
fn invalid_opcode_panics() {
    let bytes = [0xFF, 0xFF]; // opcode would be 63, which is invalid
    EffectOp::from_bytes(bytes);
}
