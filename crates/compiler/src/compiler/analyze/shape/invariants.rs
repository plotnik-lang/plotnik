//! Invariant checks excluded from coverage reports.

#![cfg_attr(coverage_nightly, coverage(off))]

use crate::compiler::parse::ast::Alternative;

#[inline]
pub fn ensure_both_labeling_kinds<'a>(
    first_labeled: Option<&'a Alternative>,
    first_unlabeled: Option<&'a Alternative>,
) -> (&'a Alternative, &'a Alternative) {
    match (first_labeled, first_unlabeled) {
        (Some(t), Some(u)) => (t, u),
        _ => panic!("mixed labeling without both labeled and unlabeled alternatives"),
    }
}
