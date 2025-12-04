//! Invariant checks excluded from coverage reports.

#![cfg_attr(coverage_nightly, coverage(off))]

use crate::parser::{Alt, Branch, Root};

#[inline]
pub fn assert_root_no_bare_exprs(root: &Root) {
    assert!(
        root.exprs().next().is_none(),
        "alt_kind: unexpected bare Expr in Root (parser should wrap in Def)"
    );
}

#[inline]
pub fn assert_alt_no_bare_exprs(alt: &Alt) {
    assert!(
        alt.exprs().next().is_none(),
        "alt_kind: unexpected bare Expr in Alt (parser should wrap in Branch)"
    );
}

#[inline]
pub fn ensure_both_branch_kinds<'a>(
    first_tagged: Option<&'a Branch>,
    first_untagged: Option<&'a Branch>,
) -> (&'a Branch, &'a Branch) {
    match (first_tagged, first_untagged) {
        (Some(t), Some(u)) => (t, u),
        _ => panic!(
            "alt_kind: Mixed alternation without both tagged and untagged branches \
             (should be caught by AltKind::compute_kind)"
        ),
    }
}
