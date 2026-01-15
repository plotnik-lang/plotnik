//! Invariant checks excluded from coverage reports.

#![cfg_attr(coverage_nightly, coverage(off))]

use super::core::Parser;
use super::cst::SyntaxKind;

impl Parser<'_, '_> {
    #[inline]
    pub(super) fn ensure_progress(&self) {
        assert!(
            self.debug_fuel.get() != 0,
            "parser is stuck: too many lookaheads"
        );
        self.debug_fuel.set(self.debug_fuel.get() - 1);
    }

    #[inline]
    pub(super) fn assert_current(&mut self, expected_kind: SyntaxKind) {
        let current_kind = self.current();
        assert_eq!(
            current_kind, expected_kind,
            "broken parser invariant: expected {:?} but found {:?} (upstream caller's responsibility)",
            expected_kind, current_kind,
        );
    }
}
