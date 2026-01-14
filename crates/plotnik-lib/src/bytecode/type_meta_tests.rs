use super::*;

#[test]
fn type_def_size() {
    assert_eq!(std::mem::size_of::<TypeDef>(), 4);
}

#[test]
fn type_member_size() {
    assert_eq!(std::mem::size_of::<TypeMember>(), 4);
}

#[test]
fn type_name_size() {
    assert_eq!(std::mem::size_of::<TypeName>(), 4);
}

#[test]
fn type_kind_is_wrapper() {
    assert!(TypeKind::Optional.is_wrapper());
    assert!(TypeKind::ArrayZeroOrMore.is_wrapper());
    assert!(TypeKind::ArrayOneOrMore.is_wrapper());
    assert!(!TypeKind::Struct.is_wrapper());
    assert!(!TypeKind::Enum.is_wrapper());
}

#[test]
fn type_kind_aliases() {
    // Test bytecode-friendly aliases
    assert_eq!(TypeKind::ARRAY_STAR, TypeKind::ArrayZeroOrMore);
    assert_eq!(TypeKind::ARRAY_PLUS, TypeKind::ArrayOneOrMore);
}
