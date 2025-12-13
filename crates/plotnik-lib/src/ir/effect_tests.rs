use super::*;

#[test]
fn effect_op_size_and_align() {
    assert_eq!(size_of::<EffectOp>(), 4);
    assert_eq!(align_of::<EffectOp>(), 2);
}

#[test]
fn effect_op_variants() {
    // Ensure all variants exist and are constructible
    let _ = EffectOp::CaptureNode;
    let _ = EffectOp::StartArray;
    let _ = EffectOp::PushElement;
    let _ = EffectOp::EndArray;
    let _ = EffectOp::StartObject;
    let _ = EffectOp::EndObject;
    let _ = EffectOp::Field(0);
    let _ = EffectOp::StartVariant(0);
    let _ = EffectOp::EndVariant;
    let _ = EffectOp::ToString;
}
