use std::num::NonZeroU16;

use super::node_type_ir::NodeTypeIR;

#[test]
fn roundtrip_any() {
    let orig = NodeTypeIR::Any;
    let (kind, type_val) = orig.to_bytes();
    let decoded = NodeTypeIR::from_bytes(kind, type_val);
    assert_eq!(decoded, orig);
}

#[test]
fn roundtrip_named_wildcard() {
    let orig = NodeTypeIR::Named(None);
    let (kind, type_val) = orig.to_bytes();
    let decoded = NodeTypeIR::from_bytes(kind, type_val);
    assert_eq!(decoded, orig);
}

#[test]
fn roundtrip_named_specific() {
    let orig = NodeTypeIR::Named(NonZeroU16::new(42));
    let (kind, type_val) = orig.to_bytes();
    let decoded = NodeTypeIR::from_bytes(kind, type_val);
    assert_eq!(decoded, orig);
}

#[test]
fn roundtrip_anonymous_wildcard() {
    let orig = NodeTypeIR::Anonymous(None);
    let (kind, type_val) = orig.to_bytes();
    let decoded = NodeTypeIR::from_bytes(kind, type_val);
    assert_eq!(decoded, orig);
}

#[test]
fn roundtrip_anonymous_specific() {
    let orig = NodeTypeIR::Anonymous(NonZeroU16::new(100));
    let (kind, type_val) = orig.to_bytes();
    let decoded = NodeTypeIR::from_bytes(kind, type_val);
    assert_eq!(decoded, orig);
}

#[test]
fn type_id_extraction() {
    assert_eq!(NodeTypeIR::Any.type_id(), None);
    assert_eq!(NodeTypeIR::Named(None).type_id(), None);
    assert_eq!(
        NodeTypeIR::Named(NonZeroU16::new(5)).type_id(),
        NonZeroU16::new(5)
    );
    assert_eq!(NodeTypeIR::Anonymous(None).type_id(), None);
    assert_eq!(
        NodeTypeIR::Anonymous(NonZeroU16::new(7)).type_id(),
        NonZeroU16::new(7)
    );
}

#[test]
fn kind_checks() {
    assert!(NodeTypeIR::Any.is_any());
    assert!(!NodeTypeIR::Any.is_named());
    assert!(!NodeTypeIR::Any.is_anonymous());

    assert!(!NodeTypeIR::Named(None).is_any());
    assert!(NodeTypeIR::Named(None).is_named());
    assert!(!NodeTypeIR::Named(None).is_anonymous());

    assert!(!NodeTypeIR::Anonymous(None).is_any());
    assert!(!NodeTypeIR::Anonymous(None).is_named());
    assert!(NodeTypeIR::Anonymous(None).is_anonymous());
}
