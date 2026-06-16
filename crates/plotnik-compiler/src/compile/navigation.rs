//! Navigation computation for sequence and node compilation.
//!
//! Handles anchor-based navigation modes and navigation transformations
//! for quantifier repeat iterations.

use std::collections::HashSet;

use crate::analyze::symbol_table::SymbolTable;
use crate::parser::{Expr, Ref, SeqItem};
use plotnik_bytecode::Nav;

/// Classifies whether expressions may match anonymous nodes after syntactic wrappers.
pub struct AnonymousClassifier<'a> {
    symbol_table: &'a SymbolTable,
}

fn expr_has_direct_alt_branch_nav(expr: &Expr) -> bool {
    match expr {
        Expr::AltExpr(_) => true,
        Expr::CapturedExpr(cap) => cap
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

    pub fn expr_may_match_anonymous(&self, expr: Option<&Expr>) -> bool {
        let mut visited = HashSet::new();
        expr.is_some_and(|expr| self.expr_may_match_anonymous_inner(expr, &mut visited))
    }

    fn expr_may_match_anonymous_inner(&self, expr: &Expr, visited: &mut HashSet<String>) -> bool {
        match expr {
            Expr::AnonymousNode(_) => true,
            Expr::CapturedExpr(cap) => cap
                .inner()
                .as_ref()
                .is_some_and(|inner| self.expr_may_match_anonymous_inner(inner, visited)),
            Expr::QuantifiedExpr(q) => q
                .inner()
                .as_ref()
                .is_some_and(|inner| self.expr_may_match_anonymous_inner(inner, visited)),
            Expr::FieldExpr(field) => field
                .value()
                .as_ref()
                .is_some_and(|value| self.expr_may_match_anonymous_inner(value, visited)),
            Expr::AltExpr(alt) => alt
                .branches()
                .filter_map(|branch| branch.body())
                .any(|body| self.expr_may_match_anonymous_inner(&body, visited)),
            Expr::SeqExpr(seq) => seq
                .children()
                .any(|child| self.expr_may_match_anonymous_inner(&child, visited)),
            Expr::Ref(r) => self.ref_may_match_anonymous(r, visited),
            Expr::NamedNode(_) => false,
        }
    }

    fn ref_may_match_anonymous(&self, r: &Ref, visited: &mut HashSet<String>) -> bool {
        let Some(name_token) = r.name() else {
            return false;
        };
        let name = name_token.text();

        if !visited.insert(name.to_owned()) {
            return false;
        }

        let result = self
            .symbol_table
            .get(name)
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

        let prev_expr = items.iter().rev().skip(1).find_map(|item| {
            if let SeqItem::Expr(e) = item {
                Some(e)
            } else {
                None
            }
        });

        let classifier = AnonymousClassifier::new(symbol_table);
        let nav = if classifier.expr_may_match_anonymous(prev_expr) {
            Nav::UpSkipExtras(1)
        } else {
            Nav::UpSkipTrivia(1)
        };
        return (true, Some(nav));
    }

    if items.len() == 1
        && let Some(SeqItem::Expr(Expr::SeqExpr(seq))) = items.first()
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
    let mut is_first_expr = true;
    let classifier = AnonymousClassifier::new(symbol_table);

    for (idx, item) in items.iter().enumerate() {
        match item {
            SeqItem::Anchor(anchor) => {
                pending_anchor_strict = Some(anchor.is_strict());
            }
            SeqItem::Expr(expr) => {
                let current_is_anonymous = classifier.expr_may_match_anonymous(Some(expr));
                // Alternation branches compile their own entry nav, so the branch body—not
                // the whole alternation—decides whether soft anchors use extras-only nav.
                let current_is_anonymous_for_anchor = if expr_has_direct_alt_branch_nav(expr) {
                    false
                } else {
                    current_is_anonymous
                };
                let nav = if let Some(is_exact) = pending_anchor_strict {
                    if is_first_expr && is_inside_node {
                        Some(if is_exact {
                            Nav::DownExact
                        } else if current_is_anonymous_for_anchor {
                            Nav::DownSkipExtras
                        } else {
                            Nav::DownSkip
                        })
                    } else if !is_first_expr {
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
                } else if !is_first_expr {
                    Some(Nav::Next)
                } else {
                    None
                };

                result.push((idx, nav));
                pending_anchor_strict = None;
                prev_is_anonymous = current_is_anonymous;
                is_first_expr = false;
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
pub fn expr_owns_iteration(expr: &Expr) -> bool {
    quantifier_operator_kind(expr).is_some()
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

/// Unwraps CapturedExpr wrappers before testing for a quantifier operator.
fn quantifier_operator_kind(expr: &Expr) -> Option<crate::parser::SyntaxKind> {
    let expr = match expr {
        Expr::CapturedExpr(cap) => cap.inner()?,
        e => e.clone(),
    };

    let Expr::QuantifiedExpr(q) = &expr else {
        return None;
    };
    Some(q.operator()?.kind())
}

pub fn is_skippable_quantifier(expr: &Expr) -> bool {
    use crate::parser::SyntaxKind;
    quantifier_operator_kind(expr).is_some_and(|k| {
        matches!(
            k,
            SyntaxKind::Question
                | SyntaxKind::QuestionQuestion
                | SyntaxKind::Star
                | SyntaxKind::StarQuestion
        )
    })
}
