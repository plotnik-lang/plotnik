use super::cst::SyntaxKind::*;
use super::token_set::TokenSet;

#[test]
fn test_token_set_contains() {
    let set = TokenSet::new(&[ParenOpen, ParenClose, Star]);
    assert!(set.contains(ParenOpen));
    assert!(set.contains(ParenClose));
    assert!(set.contains(Star));
    assert!(!set.contains(Plus));
    assert!(!set.contains(Colon));
}
