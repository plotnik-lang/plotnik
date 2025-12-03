//! Invariant checks excluded from coverage reports.

#![cfg_attr(coverage_nightly, coverage(off))]

use super::core::Parser;
use crate::ast::syntax_kind::SyntaxKind;

impl Parser<'_> {
    #[inline]
    #[cfg(debug_assertions)]
    pub(super) fn assert_progress(&self) {
        if let Some(limit) = self.debug_fuel_limit
            && self.debug_fuel.get() == 0
        {
            panic!("parser is stuck: no progress made in {limit} iterations");
        }
    }

    #[inline]
    pub(super) fn assert_equals_eaten(&self, ate_equals: bool) {
        if !ate_equals {
            panic!(
                "parse_def: expected '=' but found {:?} (caller should verify Equals is present)",
                self.current()
            );
        }
    }

    #[inline]
    pub(super) fn assert_string_quote_match(&self, actual: SyntaxKind, expected: SyntaxKind) {
        if actual != expected {
            panic!(
                "bump_string_tokens: expected closing {:?} but found {:?} \
                 (lexer should only produce quote tokens from complete strings)",
                expected, actual
            );
        }
    }

    #[inline]
    pub(super) fn assert_id_token(&self, kind: SyntaxKind) {
        if kind != SyntaxKind::Id {
            panic!(
                "parse_field: expected Id but found {:?} (caller should verify Id is present)",
                kind
            );
        }
    }
}

#[inline]
pub(super) fn assert_nonempty(s: &str) {
    if s.is_empty() {
        panic!("capitalize_first: called with empty string");
    }
}
