//! Navigation computation for sequence and node compilation.
//!
//! Handles anchor-based navigation modes and navigation transformations
//! for quantifier repeat iterations.

use crate::parser::{Expr, SeqItem};
use plotnik_bytecode::Nav;

// Re-export from parser for compile module consumers
pub use crate::parser::is_truly_empty_scope;

/// Check if an expression is anonymous (string literal or wildcard).
pub fn expr_is_anonymous(expr: Option<&Expr>) -> bool {
    matches!(expr, Some(Expr::AnonymousNode(_)))
}

/// Check for trailing anchor in items, looking inside sequences if needed.
/// Returns (has_trailing_anchor, is_strict).
pub fn check_trailing_anchor(items: &[SeqItem]) -> (bool, bool) {
    // Direct trailing anchor
    if matches!(items.last(), Some(SeqItem::Anchor(_))) {
        let prev_expr = items.iter().rev().skip(1).find_map(|item| {
            if let SeqItem::Expr(e) = item {
                Some(e)
            } else {
                None
            }
        });
        return (true, expr_is_anonymous(prev_expr));
    }

    // Check if only child is a sequence with trailing anchor
    if items.len() == 1
        && let Some(SeqItem::Expr(Expr::SeqExpr(seq))) = items.first()
    {
        let seq_items: Vec<_> = seq.items().collect();
        return check_trailing_anchor(&seq_items);
    }

    (false, false)
}

/// Compute navigation modes for each expression based on anchor context.
/// Returns a vector of (expression index, nav mode) pairs.
pub fn compute_nav_modes(items: &[SeqItem], is_inside_node: bool) -> Vec<(usize, Option<Nav>)> {
    let mut result = Vec::new();
    let mut pending_anchor = false;
    let mut prev_is_anonymous = false;
    let mut is_first_expr = true;

    for (idx, item) in items.iter().enumerate() {
        match item {
            SeqItem::Anchor(_) => {
                pending_anchor = true;
            }
            SeqItem::Expr(expr) => {
                let current_is_anonymous = matches!(expr, Expr::AnonymousNode(_));
                let nav = if pending_anchor {
                    // Anchor between previous item and this one
                    let is_exact = prev_is_anonymous || current_is_anonymous;
                    if is_first_expr && is_inside_node {
                        // First child with leading anchor
                        Some(if is_exact {
                            Nav::DownExact
                        } else {
                            Nav::DownSkip
                        })
                    } else if !is_first_expr {
                        // Sibling with anchor
                        Some(if is_exact {
                            Nav::NextExact
                        } else {
                            Nav::NextSkip
                        })
                    } else {
                        // First in sequence (not inside node)
                        None
                    }
                } else if !is_first_expr {
                    // Normal sibling navigation (no anchor)
                    Some(Nav::Next)
                } else {
                    // First expression - use default (None for sequences, Down for nodes)
                    None
                };

                result.push((idx, nav));
                pending_anchor = false;
                prev_is_anonymous = current_is_anonymous;
                is_first_expr = false;
            }
        }
    }

    result
}

/// Compute navigation for repeat iterations in quantifiers.
///
/// When a quantifier repeats, it needs to advance to the next sibling:
/// - First iteration uses `nav_override` (e.g., Down into parent's children)
/// - Repeat iterations must use Next to advance to subsequent siblings
pub fn repeat_nav_for(first_nav: Option<Nav>) -> Option<Nav> {
    match first_nav {
        Some(Nav::Down) => Some(Nav::Next),
        Some(Nav::DownSkip) => Some(Nav::NextSkip),
        Some(Nav::DownExact) => Some(Nav::NextExact),
        Some(nav @ (Nav::Next | Nav::NextSkip | Nav::NextExact)) => Some(nav),
        None | Some(Nav::Stay) => Some(Nav::Next),
        _ => None,
    }
}

/// Check if navigation is a Down variant (descends into children).
pub fn is_down_nav(nav: Option<Nav>) -> bool {
    matches!(nav, Some(Nav::Down | Nav::DownSkip | Nav::DownExact))
}

/// Extract the operator kind from an expression if it's a quantifier.
/// Unwraps CapturedExpr if present.
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

/// Check if expression is optional (?) or star (*) - patterns that can match zero times.
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

/// Syntactic check for star/plus quantifier (fallback when type info unavailable).
pub fn is_star_or_plus_quantifier(expr: Option<&Expr>) -> bool {
    use crate::parser::SyntaxKind;
    expr.and_then(quantifier_operator_kind).is_some_and(|k| {
        matches!(
            k,
            SyntaxKind::Star
                | SyntaxKind::StarQuestion
                | SyntaxKind::Plus
                | SyntaxKind::PlusQuestion
        )
    })
}

/// Determines if an expression creates a scope boundary when captured.
/// Sequences and alternations create scopes; named nodes/refs don't.
pub fn inner_creates_scope(inner: &Expr) -> bool {
    match inner {
        Expr::SeqExpr(_) | Expr::AltExpr(_) => true,
        Expr::QuantifiedExpr(q) => q.inner().is_some_and(|i| inner_creates_scope(&i)),
        _ => false,
    }
}
