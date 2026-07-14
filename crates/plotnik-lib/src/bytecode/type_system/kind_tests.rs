use super::*;

#[test]
fn from_u8_valid() {
    assert_eq!(TypeKind::from_u8(0), Some(TypeKind::NoValue));
    assert_eq!(TypeKind::from_u8(1), Some(TypeKind::Node));
    assert_eq!(TypeKind::from_u8(2), Some(TypeKind::Option));
    assert_eq!(TypeKind::from_u8(3), Some(TypeKind::ListZeroOrMore));
    assert_eq!(TypeKind::from_u8(4), Some(TypeKind::ListOneOrMore));
    assert_eq!(TypeKind::from_u8(5), Some(TypeKind::Record));
    assert_eq!(TypeKind::from_u8(6), Some(TypeKind::Variant));
    assert_eq!(TypeKind::from_u8(7), Some(TypeKind::Alias));
    assert_eq!(TypeKind::from_u8(8), Some(TypeKind::Text));
    assert_eq!(TypeKind::from_u8(9), Some(TypeKind::Bool));
}

#[test]
fn from_u8_invalid() {
    assert_eq!(TypeKind::from_u8(10), None);
    assert_eq!(TypeKind::from_u8(255), None);
}

#[test]
fn is_primitive() {
    assert!(TypeKind::NoValue.is_primitive());
    assert!(TypeKind::Node.is_primitive());
    assert!(TypeKind::Text.is_primitive());
    assert!(TypeKind::Bool.is_primitive());
    assert!(!TypeKind::Option.is_primitive());
    assert!(!TypeKind::Record.is_primitive());
}

#[test]
fn is_wrapper() {
    assert!(TypeKind::Option.is_wrapper());
    assert!(TypeKind::ListZeroOrMore.is_wrapper());
    assert!(TypeKind::ListOneOrMore.is_wrapper());
    assert!(!TypeKind::Record.is_wrapper());
    assert!(!TypeKind::Variant.is_wrapper());
    assert!(!TypeKind::Alias.is_wrapper());
    assert!(!TypeKind::NoValue.is_wrapper());
}

#[test]
fn is_list() {
    assert!(!TypeKind::Option.is_list());
    assert!(TypeKind::ListZeroOrMore.is_list());
    assert!(TypeKind::ListOneOrMore.is_list());
    assert!(!TypeKind::Record.is_list());
    assert!(!TypeKind::Variant.is_list());
    assert!(!TypeKind::Alias.is_list());
}

#[test]
fn is_alias() {
    assert!(!TypeKind::Option.is_alias());
    assert!(!TypeKind::ListZeroOrMore.is_alias());
    assert!(!TypeKind::ListOneOrMore.is_alias());
    assert!(!TypeKind::Record.is_alias());
    assert!(!TypeKind::Variant.is_alias());
    assert!(TypeKind::Alias.is_alias());
}

#[test]
fn primitive_name() {
    assert_eq!(TypeKind::NoValue.primitive_name(), Some("NoValue"));
    assert_eq!(TypeKind::Node.primitive_name(), Some("Node"));
    assert_eq!(TypeKind::Text.primitive_name(), Some("Text"));
    assert_eq!(TypeKind::Bool.primitive_name(), Some("Bool"));
    assert_eq!(TypeKind::Record.primitive_name(), None);
}
