use super::*;

#[test]
fn byte_roundtrip() {
    for op in [
        PredicateOp::Eq,
        PredicateOp::Ne,
        PredicateOp::StartsWith,
        PredicateOp::EndsWith,
        PredicateOp::Contains,
        PredicateOp::RegexMatch,
        PredicateOp::RegexNoMatch,
    ] {
        assert_eq!(PredicateOp::from_byte(op.to_byte()), op);
    }
}

#[test]
fn is_regex_op() {
    assert!(!PredicateOp::Eq.is_regex_op());
    assert!(!PredicateOp::Ne.is_regex_op());
    assert!(!PredicateOp::StartsWith.is_regex_op());
    assert!(!PredicateOp::EndsWith.is_regex_op());
    assert!(!PredicateOp::Contains.is_regex_op());
    assert!(PredicateOp::RegexMatch.is_regex_op());
    assert!(PredicateOp::RegexNoMatch.is_regex_op());
}
