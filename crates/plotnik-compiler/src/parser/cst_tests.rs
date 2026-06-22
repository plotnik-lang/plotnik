use super::cst::{SyntaxKind::*, TokenSet};

#[test]
fn test_token_set_contains() {
    let set = TokenSet::new(&[ParenOpen, ParenClose, Star]);
    assert!(set.contains(ParenOpen));
    assert!(set.contains(ParenClose));
    assert!(set.contains(Star));
    assert!(!set.contains(Plus));
    assert!(!set.contains(Colon));
}

#[test]
fn test_token_set_debug() {
    let set = TokenSet::new(&[ParenOpen, Star, Plus]);
    let debug_str = format!("{:?}", set);
    assert!(debug_str.contains("ParenOpen"));
    assert!(debug_str.contains("Star"));
    assert!(debug_str.contains("Plus"));
}
