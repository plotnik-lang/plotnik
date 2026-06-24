use super::ast::QuantifierKind;

#[test]
fn is_non_empty() {
    assert!(!QuantifierKind::Optional.is_non_empty());
    assert!(!QuantifierKind::ZeroOrMore.is_non_empty());
    assert!(QuantifierKind::OneOrMore.is_non_empty());
}
