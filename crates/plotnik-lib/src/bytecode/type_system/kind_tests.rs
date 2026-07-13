use super::*;

#[test]
fn from_u8_valid() {
    assert_eq!(TypeKind::from_u8(0), Some(TypeKind::Void));
    assert_eq!(TypeKind::from_u8(1), Some(TypeKind::Node));
    assert_eq!(TypeKind::from_u8(2), Some(TypeKind::Optional));
    assert_eq!(TypeKind::from_u8(3), Some(TypeKind::ArrayZeroOrMore));
    assert_eq!(TypeKind::from_u8(4), Some(TypeKind::ArrayOneOrMore));
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
    assert!(TypeKind::Void.is_primitive());
    assert!(TypeKind::Node.is_primitive());
    assert!(TypeKind::Text.is_primitive());
    assert!(TypeKind::Bool.is_primitive());
    assert!(!TypeKind::Optional.is_primitive());
    assert!(!TypeKind::Record.is_primitive());
}

#[test]
fn is_wrapper() {
    assert!(TypeKind::Optional.is_wrapper());
    assert!(TypeKind::ArrayZeroOrMore.is_wrapper());
    assert!(TypeKind::ArrayOneOrMore.is_wrapper());
    assert!(!TypeKind::Record.is_wrapper());
    assert!(!TypeKind::Variant.is_wrapper());
    assert!(!TypeKind::Alias.is_wrapper());
    assert!(!TypeKind::Void.is_wrapper());
}

#[test]
fn is_array() {
    assert!(!TypeKind::Optional.is_array());
    assert!(TypeKind::ArrayZeroOrMore.is_array());
    assert!(TypeKind::ArrayOneOrMore.is_array());
    assert!(!TypeKind::Record.is_array());
    assert!(!TypeKind::Variant.is_array());
    assert!(!TypeKind::Alias.is_array());
}

#[test]
fn is_non_empty_array() {
    assert!(!TypeKind::ArrayZeroOrMore.is_non_empty_array());
    assert!(TypeKind::ArrayOneOrMore.is_non_empty_array());
}

#[test]
fn is_alias() {
    assert!(!TypeKind::Optional.is_alias());
    assert!(!TypeKind::ArrayZeroOrMore.is_alias());
    assert!(!TypeKind::ArrayOneOrMore.is_alias());
    assert!(!TypeKind::Record.is_alias());
    assert!(!TypeKind::Variant.is_alias());
    assert!(TypeKind::Alias.is_alias());
}

#[test]
fn primitive_name() {
    assert_eq!(TypeKind::Void.primitive_name(), Some("Void"));
    assert_eq!(TypeKind::Node.primitive_name(), Some("Node"));
    assert_eq!(TypeKind::Text.primitive_name(), Some("Text"));
    assert_eq!(TypeKind::Bool.primitive_name(), Some("Bool"));
    assert_eq!(TypeKind::Record.primitive_name(), None);
}
