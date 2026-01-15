use super::*;

#[test]
fn combine_arities() {
    assert_eq!(Arity::One.combine(Arity::One), Arity::One);
    assert_eq!(Arity::One.combine(Arity::Many), Arity::Many);
    assert_eq!(Arity::Many.combine(Arity::One), Arity::Many);
    assert_eq!(Arity::Many.combine(Arity::Many), Arity::Many);
}

#[test]
fn is_one_and_many() {
    assert!(Arity::One.is_one());
    assert!(!Arity::One.is_many());
    assert!(!Arity::Many.is_one());
    assert!(Arity::Many.is_many());
}
