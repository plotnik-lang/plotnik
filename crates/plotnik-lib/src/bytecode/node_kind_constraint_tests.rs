use crate::core::NodeKindId;

use super::node_kind_constraint::NodeKindConstraint;

#[test]
fn roundtrip_any() {
    let orig = NodeKindConstraint::Any;
    let (kind, type_val) = orig.to_bytes();
    let decoded = NodeKindConstraint::from_bytes(kind, type_val);
    assert_eq!(decoded, orig);
}

#[test]
fn roundtrip_named_wildcard() {
    let orig = NodeKindConstraint::Named(None);
    let (kind, type_val) = orig.to_bytes();
    let decoded = NodeKindConstraint::from_bytes(kind, type_val);
    assert_eq!(decoded, orig);
}

#[test]
fn roundtrip_named_specific() {
    let orig = NodeKindConstraint::Named(NodeKindId::try_from(42).ok());
    let (kind, type_val) = orig.to_bytes();
    let decoded = NodeKindConstraint::from_bytes(kind, type_val);
    assert_eq!(decoded, orig);
}

#[test]
fn roundtrip_anonymous_wildcard() {
    let orig = NodeKindConstraint::Anonymous(None);
    let (kind, type_val) = orig.to_bytes();
    let decoded = NodeKindConstraint::from_bytes(kind, type_val);
    assert_eq!(decoded, orig);
}

#[test]
fn roundtrip_anonymous_specific() {
    let orig = NodeKindConstraint::Anonymous(NodeKindId::try_from(100).ok());
    let (kind, type_val) = orig.to_bytes();
    let decoded = NodeKindConstraint::from_bytes(kind, type_val);
    assert_eq!(decoded, orig);
}

#[test]
fn type_id_extraction() {
    assert_eq!(NodeKindConstraint::Any.kind_id(), None);
    assert_eq!(NodeKindConstraint::Named(None).kind_id(), None);
    assert_eq!(
        NodeKindConstraint::Named(NodeKindId::try_from(5).ok()).kind_id(),
        NodeKindId::try_from(5).ok()
    );
    assert_eq!(NodeKindConstraint::Anonymous(None).kind_id(), None);
    assert_eq!(
        NodeKindConstraint::Anonymous(NodeKindId::try_from(7).ok()).kind_id(),
        NodeKindId::try_from(7).ok()
    );
}

#[test]
fn kind_checks() {
    assert!(NodeKindConstraint::Any.is_any());
    assert!(!NodeKindConstraint::Any.is_named());
    assert!(!NodeKindConstraint::Any.is_anonymous());

    assert!(!NodeKindConstraint::Named(None).is_any());
    assert!(NodeKindConstraint::Named(None).is_named());
    assert!(!NodeKindConstraint::Named(None).is_anonymous());

    assert!(!NodeKindConstraint::Anonymous(None).is_any());
    assert!(!NodeKindConstraint::Anonymous(None).is_named());
    assert!(NodeKindConstraint::Anonymous(None).is_anonymous());
}
