use crate::bytecode::TypeKind;

#[test]
fn type_kind_aliases() {
    // Test bytecode-friendly aliases
    assert_eq!(TypeKind::ARRAY_STAR, TypeKind::ArrayZeroOrMore);
    assert_eq!(TypeKind::ARRAY_PLUS, TypeKind::ArrayOneOrMore);
}
