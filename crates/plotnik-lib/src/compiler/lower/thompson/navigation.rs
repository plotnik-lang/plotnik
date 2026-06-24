//! Navigation computation for sequence and node compilation.
//!
//! Handles anchor-based navigation modes and navigation transformations
//! for quantifier repeat iterations.

use std::collections::HashSet;

use crate::bytecode::Nav;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::parse::ast::{Pattern, DefRef, SeqItem};

/// Classifies whether expressions may match anonymous nodes after syntactic wrappers.
pub struct AnonymousClassifier<'a> {
    symbol_table: &'a SymbolTable,
}

fn expr_has_direct_alt_branch_nav(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Union(_) | Pattern::Enum(_) => true,
        Pattern::CapturedPattern(cap) => cap
            .inner()
            .as_ref()
            .is_some_and(expr_has_direct_alt_branch_nav),
        _ => false,
    }
}

impl<'a> AnonymousClassifier<'a> {
    pub fn new(symbol_table: &'a SymbolTable) -> Self {
        Self { symbol_table }
    }

    pub fn expr_may_match_anonymous(&self, pattern: Option<&Pattern>) -> bool {
        let mut visited = HashSet::new();
        pattern.is_some_and(|pattern| self.expr_may_match_anonymous_inner(pattern, &mut visited))
    }

    fn expr_may_match_anonymous_inner(
        &self,
        pattern: &Pattern,
        visited: &mut HashSet<String>,
    ) -> bool {
        match pattern {
            Pattern::TokenPattern(_) => true,
            Pattern::CapturedPattern(cap) => cap
                .inner()
                .as_ref()
                .is_some_and(|inner| self.expr_may_match_anonymous_inner(inner, visited)),
            Pattern::QuantifiedPattern(q) => q
                .inner()
                .as_ref()
                .is_some_and(|inner| self.expr_may_match_anonymous_inner(inner, visited)),
            Pattern::FieldPattern(field) => field
                .value()
                .as_ref()
                .is_some_and(|value| self.expr_may_match_anonymous_inner(value, visited)),
            Pattern::Union(_) | Pattern::Enum(_) => pattern
                .children()
                .iter()
                .any(|body| self.expr_may_match_anonymous_inner(body, visited)),
            Pattern::SeqPattern(seq) => seq
                .children()
                .any(|child| self.expr_may_match_anonymous_inner(&child, visited)),
            Pattern::DefRef(r) => self.ref_may_match_anonymous(r, visited),
            Pattern::NodePattern(_) => false,
        }
    }

    fn ref_may_match_anonymous(&self, r: &DefRef, visited: &mut HashSet<String>) -> bool {
        let Some(name_token) = r.name() else {
            return false;
        };
        let name = name_token.text();

        if !visited.insert(name.to_owned()) {
            return false;
        }

        let result = self
            .symbol_table
            .body(name)
            .is_some_and(|body| self.expr_may_match_anonymous_inner(body, visited));

        visited.remove(name);
        result
    }
}

/// Check for trailing anchor in items, descending into a sole-child sequence if needed.
pub fn check_trailing_anchor(items: &[SeqItem], symbol_table: &SymbolTable) -> (bool, Option<Nav>) {
    if let Some(SeqItem::Anchor(anchor)) = items.last() {
        if anchor.is_strict() {
            return (true, Some(Nav::UpExact(1)));
        }

        let prev_pattern = items.iter().rev().skip(1).find_map(|item| {
            if let SeqItem::Pattern(e) = item {
                Some(e)
            } else {
                None
            }
        });

        let classifier = AnonymousClassifier::new(symbol_table);
        let nav = if classifier.expr_may_match_anonymous(prev_pattern) {
            Nav::UpSkipExtras(1)
        } else {
            Nav::UpSkipTrivia(1)
        };
        return (true, Some(nav));
    }

    if items.len() == 1
        && let Some(SeqItem::Pattern(Pattern::SeqPattern(seq))) = items.first()
    {
        let seq_items: Vec<_> = seq.items().collect();
        return check_trailing_anchor(&seq_items, symbol_table);
    }

    (false, None)
}

pub fn compute_nav_modes(
    items: &[SeqItem],
    is_inside_node: bool,
    symbol_table: &SymbolTable,
) -> Vec<(usize, Option<Nav>)> {
    let mut result = Vec::new();
    let mut pending_anchor_strict = None;
    let mut prev_is_anonymous = false;
    let mut is_first_pattern = true;
    let classifier = AnonymousClassifier::new(symbol_table);

    for (idx, item) in items.iter().enumerate() {
        match item {
            SeqItem::Anchor(anchor) => {
                pending_anchor_strict = Some(anchor.is_strict());
            }
            SeqItem::Pattern(pattern) => {
                let current_is_anonymous = classifier.expr_may_match_anonymous(Some(pattern));
                // Alternation branches compile their own entry nav, so the branch body—not
                // the whole alternation—decides whether soft anchors use extras-only nav.
                let current_is_anonymous_for_anchor = if expr_has_direct_alt_branch_nav(pattern) {
                    false
                } else {
                    current_is_anonymous
                };
                let nav = if let Some(is_exact) = pending_anchor_strict {
                    if is_first_pattern && is_inside_node {
                        Some(if is_exact {
                            Nav::DownExact
                        } else if current_is_anonymous_for_anchor {
                            Nav::DownSkipExtras
                        } else {
                            Nav::DownSkip
                        })
                    } else if !is_first_pattern {
                        Some(if is_exact {
                            Nav::NextExact
                        } else if prev_is_anonymous || current_is_anonymous_for_anchor {
                            Nav::NextSkipExtras
                        } else {
                            Nav::NextSkip
                        })
                    } else {
                        None
                    }
                } else if !is_first_pattern {
                    Some(Nav::Next)
                } else {
                    None
                };

                result.push((idx, nav));
                pending_anchor_strict = None;
                prev_is_anonymous = current_is_anonymous;
                is_first_pattern = false;
            }
        }
    }

    result
}

/// Check if an expression compiles to a loop that owns its own sibling
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
pub fn expr_owns_iteration(pattern: &Pattern) -> bool {
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
