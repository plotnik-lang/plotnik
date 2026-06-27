//! Navigation computation for sequence and node compilation.
//!
//! Anchor-based gap semantics live in `analyze/anchors.rs`, shared with the grammar
//! satisfiability checker so the two cannot drift, and are re-exported here for codegen.
//! This module keeps the codegen-only navs that drive the VM's sibling search and quantifier
//! repeat iteration.

use crate::bytecode::Nav;
use crate::compiler::parse::ast::Pattern;

pub use crate::compiler::analyze::anchors::AnchorSemantics;

/// Check if a pattern compiles to a loop that owns its own sibling
/// iteration. Only quantifiers do: a quantifier matches a variable number of
/// siblings *starting at a fixed position*, so it must drive its own
/// advancement and must not be wrapped in a position search (that would let it
/// start past non-matching siblings, breaking adjacency).
///
/// Every other form matches a single candidate. When such an item precedes an
/// anchored follower, the sequence compiler wraps it with `emit_position_search`
/// and a `StayExact` body, so the wrapper — not the item — owns the resumable
/// search. This is the single ownership rule that replaced the old
/// per-form classification.
pub fn pattern_owns_iteration(pattern: &Pattern) -> bool {
    quantifier_kind(pattern).is_some()
}

/// Extract the navigation if a *match-once* item under it owns a resumable
/// sibling search (`SkipPolicy::Any`).
///
/// For an item that matches a single candidate (a node, ref, field, or
/// alternation branch), only `Down`/`Next`/`Stay` skip past named siblings, so
/// they have multiple candidate positions and need the resumable
/// `emit_position_search` wrapper to retry a later sibling when a following
/// pattern fails. Bounded navs (anchored, exact) skip only trivia, so they have
/// a single candidate and the VM's in-instruction `continue_search` suffices;
/// `Up` navs don't search siblings.
///
/// `StayExact` is excluded on purpose: a match-once item lands at `StayExact`
/// only when an *outer* context already positioned the cursor (a Call's resume
/// checkpoint, or an alternation/sequence wrapper), so that context — not the
/// item — owns the search. Including it here makes alternations double-wrap and
/// regresses (verified: ~19 alternation/recursion tests). A quantifier loop is
/// the deliberate exception; see `quantifier::quantifier_search_nav`.
pub fn resumable_search_nav(nav: Option<Nav>) -> Option<Nav> {
    match nav {
        Some(nav @ (Nav::Down | Nav::Next | Nav::Stay)) => Some(nav),
        _ => None,
    }
}

pub fn is_down_nav(nav: Option<Nav>) -> bool {
    matches!(
        nav,
        Some(Nav::Down | Nav::DownSkip | Nav::DownSkipExtras | Nav::DownExact)
    )
}

/// Unwraps CapturedPattern wrappers before testing for quantifier arity.
fn quantifier_kind(pattern: &Pattern) -> Option<crate::compiler::parse::ast::QuantifierKind> {
    let pattern = match pattern {
        Pattern::CapturedPattern(cap) => cap.inner()?,
        e => e.clone(),
    };

    let Pattern::QuantifiedPattern(q) = &pattern else {
        return None;
    };
    q.quantifier_kind()
}

pub fn is_skippable_quantifier(pattern: &Pattern) -> bool {
    use crate::compiler::parse::ast::QuantifierKind;
    quantifier_kind(pattern)
        .is_some_and(|kind| matches!(kind, QuantifierKind::Optional | QuantifierKind::ZeroOrMore))
}
