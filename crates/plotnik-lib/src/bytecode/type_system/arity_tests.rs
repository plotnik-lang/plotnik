use super::*;

#[test]
fn combine_arities() {
    assert_eq!(Arity::One.combine(Arity::One), Arity::One);
    assert_eq!(Arity::One.combine(Arity::Many), Arity::Many);
    assert_eq!(Arity::Many.combine(Arity::One), Arity::Many);
    assert_eq!(Arity::Many.combine(Arity::Many), Arity::Many);
}
