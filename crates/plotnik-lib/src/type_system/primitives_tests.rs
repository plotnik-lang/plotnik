use super::*;

#[test]
fn primitive_indices() {
    assert_eq!(PrimitiveType::Void.index(), 0);
    assert_eq!(PrimitiveType::Node.index(), 1);
    assert_eq!(PrimitiveType::String.index(), 2);
}

#[test]
fn from_index() {
    assert_eq!(PrimitiveType::from_index(0), Some(PrimitiveType::Void));
    assert_eq!(PrimitiveType::from_index(1), Some(PrimitiveType::Node));
    assert_eq!(PrimitiveType::from_index(2), Some(PrimitiveType::String));
    assert_eq!(PrimitiveType::from_index(3), None);
}

#[test]
fn is_builtin() {
    assert!(PrimitiveType::is_builtin(0));
    assert!(PrimitiveType::is_builtin(1));
    assert!(PrimitiveType::is_builtin(2));
    assert!(!PrimitiveType::is_builtin(3));
    assert!(!PrimitiveType::is_builtin(100));
}
