use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::rules::{Symbol, TokenSet};

#[test]
fn removed_trailing_tokens_do_not_affect_set_identity() {
    let mut with_removed_token = TokenSet::new();
    with_removed_token.insert(Symbol::terminal(2));
    with_removed_token.insert(Symbol::terminal(130));

    let mut direct = TokenSet::new();
    direct.insert(Symbol::terminal(2));

    let removed = with_removed_token.remove(&Symbol::terminal(130));

    assert!(removed);
    assert_eq!(with_removed_token, direct);
    assert_eq!(hash(&with_removed_token), hash(&direct));
    assert_eq!(with_removed_token.cmp(&direct), std::cmp::Ordering::Equal);
}

#[test]
fn inserting_all_bits_reports_new_tokens_once() {
    let mut tokens = TokenSet::new();
    tokens.insert(Symbol::terminal(1));

    let mut additional = TokenSet::new();
    additional.insert(Symbol::terminal(65));

    let inserted = tokens.insert_all_terminals(&additional);
    let inserted_again = tokens.insert_all_terminals(&additional);

    assert!(inserted);
    assert!(!inserted_again);
    assert!(tokens.contains_terminal(1));
    assert!(tokens.contains_terminal(65));
    assert_eq!(tokens.len(), 2);
}

fn hash(value: &TokenSet) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
