//! Invariant checks excluded from coverage reports.

#![cfg_attr(coverage_nightly, coverage(off))]

use super::core::Parser;

impl Parser<'_> {
    #[inline]
    pub(super) fn ensure_progress(&self) {
        assert!(
            self.debug_fuel.get() != 0,
            "parser is stuck: too many lookaheads"
        );
        self.debug_fuel.set(self.debug_fuel.get() - 1);
    }
}
