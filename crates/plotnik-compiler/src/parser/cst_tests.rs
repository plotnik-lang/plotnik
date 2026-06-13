use super::cst::{QLang, SyntaxKind::*, TokenSet};
use rowan::Language;

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
fn test_is_trivia() {
    assert!(Whitespace.is_trivia());
    assert!(Newline.is_trivia());
    assert!(LineComment.is_trivia());
    assert!(BlockComment.is_trivia());
    assert!(!ParenOpen.is_trivia());
    assert!(!Error.is_trivia());
}

#[test]
fn test_syntax_kind_count_under_128() {
    assert!(
        (__LAST as u16) < 128,
        "SyntaxKind has {} variants, exceeds TokenSet capacity of 128",
        __LAST as u16
    );
}

#[test]
fn test_is_error() {
    assert!(Error.is_error());
    assert!(Garbage.is_error());
    assert!(!ParenOpen.is_error());
    assert!(!Hash.is_error());
    assert!(!Id.is_error());
    assert!(!Whitespace.is_error());
}

#[test]
fn test_token_set_debug() {
    let set = TokenSet::new(&[ParenOpen, Star, Plus]);
    let debug_str = format!("{:?}", set);
    assert!(debug_str.contains("ParenOpen"));
    assert!(debug_str.contains("Star"));
    assert!(debug_str.contains("Plus"));
}

#[test]
fn test_qlang_roundtrip() {
    for kind in [ParenOpen, ParenClose, Star, Plus, Id, Error, Whitespace] {
        let raw = QLang::kind_to_raw(kind);
        let back = QLang::kind_from_raw(raw);
        assert_eq!(kind, back);
    }
}
