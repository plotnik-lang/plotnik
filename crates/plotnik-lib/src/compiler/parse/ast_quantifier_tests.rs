use super::ast::{QuantifierGreediness, QuantifierKind, QuantifierOperator};

#[test]
fn is_non_empty() {
    assert!(!QuantifierKind::Optional.is_non_empty());
    assert!(!QuantifierKind::ZeroOrMore.is_non_empty());
    assert!(QuantifierKind::OneOrMore.is_non_empty());
}

#[test]
fn operator_tracks_arity_and_greediness() {
    let op = QuantifierOperator::new(QuantifierKind::ZeroOrMore, QuantifierGreediness::NonGreedy);

    assert_eq!(op.kind(), QuantifierKind::ZeroOrMore);
    assert!(!op.is_greedy());
}
