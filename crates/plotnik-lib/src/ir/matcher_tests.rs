use super::*;

#[test]
fn matcher_size_and_alignment() {
    assert_eq!(size_of::<Matcher>(), 16);
    assert_eq!(align_of::<Matcher>(), 4);
}

#[test]
fn consumes_node() {
    assert!(!Matcher::Epsilon.consumes_node());
    assert!(Matcher::Wildcard.consumes_node());

    let node_matcher = Matcher::Node {
        kind: 42,
        field: None,
        negated_fields: Slice::empty(),
    };
    assert!(node_matcher.consumes_node());

    let anon_matcher = Matcher::Anonymous {
        kind: 1,
        field: None,
        negated_fields: Slice::empty(),
    };
    assert!(anon_matcher.consumes_node());
}
