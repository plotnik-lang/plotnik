use super::*;

#[test]
fn requires_row_capture() {
    assert!(!QuantifierKind::Optional.requires_row_capture());
    assert!(QuantifierKind::ZeroOrMore.requires_row_capture());
    assert!(QuantifierKind::OneOrMore.requires_row_capture());
}

#[test]
fn is_non_empty() {
    assert!(!QuantifierKind::Optional.is_non_empty());
    assert!(!QuantifierKind::ZeroOrMore.is_non_empty());
    assert!(QuantifierKind::OneOrMore.is_non_empty());
}

#[test]
fn can_be_empty() {
    assert!(QuantifierKind::Optional.can_be_empty());
    assert!(QuantifierKind::ZeroOrMore.can_be_empty());
    assert!(!QuantifierKind::OneOrMore.can_be_empty());
}
