//! Invariant checks excluded from coverage reports.

#![cfg_attr(coverage_nightly, coverage(off))]

use crate::compiler::parse::ast::Branch;

#[inline]
pub fn ensure_both_branch_kinds<'a>(
    first_enum: Option<&'a Branch>,
    first_union: Option<&'a Branch>,
) -> (&'a Branch, &'a Branch) {
    match (first_enum, first_union) {
        (Some(t), Some(u)) => (t, u),
        _ => panic!(
            "alt_kind: Mixed alternation without both enum and union branches \
             (classify_alt returns Mixed only when both are present)"
        ),
    }
}
