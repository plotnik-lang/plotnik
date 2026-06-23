//! Tests for regex DFA deserialization.

use crate::bytecode::dfa::deserialize_dfa;

// `Module::load_regex_dfas` no longer pre-checks for empty DFA bytes; it relies
// on `deserialize_dfa` rejecting empty/garbage input so a forged module with a
// zero-length regex entry still fails to `InvalidRegexDfa` instead of caching a
// bogus automaton. These pin that contract.

#[test]
fn rejects_empty_bytes() {
    assert!(deserialize_dfa(&[]).is_err());
}

#[test]
fn rejects_truncated_bytes() {
    assert!(deserialize_dfa(&[0u8; 8]).is_err());
}
