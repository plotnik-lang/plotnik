//! Invariant checks excluded from coverage reports.

#![cfg_attr(coverage_nightly, coverage(off))]

use crate::ast::{Alt, Branch, Ref, Root, SyntaxNode, SyntaxToken};

#[inline]
pub fn assert_root_no_bare_exprs(root: &Root) {
    if root.exprs().next().is_some() {
        panic!("alt_kind: unexpected bare Expr in Root (parser should wrap in Def)");
    }
}

#[inline]
pub fn assert_alt_no_bare_exprs(alt: &Alt) {
    if alt.exprs().next().is_some() {
        panic!("alt_kind: unexpected bare Expr in Alt (parser should wrap in Branch)");
    }
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

#[inline]
pub fn panic_unexpected_node(node: &SyntaxNode) -> ! {
    panic!(
        "shape_cardinality: unexpected non-Expr node kind {:?} at {:?}",
        node.kind(),
        node.text_range()
    );
}

#[inline]
pub fn ensure_ref_name(r: &Ref) -> SyntaxToken {
    r.name().unwrap_or_else(|| {
        panic!(
            "shape_cardinality: Ref node missing name token at {:?} (should be caught by parser)",
            r.syntax().text_range()
        )
    })
}

#[inline]
pub fn assert_ref_in_symbols(name: &str, r: &Ref, found: bool) {
    if !found {
        panic!(
            "shape_cardinality: Ref `{}` not in symbol table at {:?} (should be caught by resolution)",
            name,
            r.syntax().text_range()
        );
    }
}

#[inline]
pub fn ensure_ref_body<'a>(name: &str, r: &Ref, body: Option<&'a SyntaxNode>) -> &'a SyntaxNode {
    body.unwrap_or_else(|| {
        panic!(
            "shape_cardinality: Ref `{}` in symbol table but no body found at {:?}",
            name,
            r.syntax().text_range()
        )
    })
}
