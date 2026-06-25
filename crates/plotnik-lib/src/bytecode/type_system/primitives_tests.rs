use super::*;

#[test]
fn primitive_indices() {
    assert_eq!(PrimitiveType::Void.index(), 0);
    assert_eq!(PrimitiveType::Node.index(), 1);
}

#[test]
fn from_index() {
    assert_eq!(PrimitiveType::from_index(0), Some(PrimitiveType::Void));
    assert_eq!(PrimitiveType::from_index(1), Some(PrimitiveType::Node));
    assert_eq!(PrimitiveType::from_index(2), None);
}

#[test]
fn is_builtin() {
    assert!(PrimitiveType::is_builtin(0));
    assert!(PrimitiveType::is_builtin(1));
    assert!(!PrimitiveType::is_builtin(2));
    assert!(!PrimitiveType::is_builtin(100));
}
