//! Quantifier-Induced Scope (QIS) detection.
//!
//! QIS triggers when a quantified expression has ≥2 propagating captures.
//! This creates an implicit object scope so captures stay coupled per-iteration.
//!
//! See ADR-0009 for full specification.

use crate::parser::{ast, token_src};

use super::{QisTrigger, Query};

impl<'a> Query<'a> {
    /// Detect Quantifier-Induced Scope triggers.
    ///
    /// QIS triggers when a quantified expression has ≥2 propagating captures
    /// (captures not absorbed by inner scopes like `{...} @x` or `[A: ...] @x`).
    pub(super) fn detect_qis(&mut self) {
        let bodies: Vec<_> = self.symbol_table.values().cloned().collect();
        for body in &bodies {
            self.detect_qis_in_expr(body);
        }
    }

    fn detect_qis_in_expr(&mut self, expr: &ast::Expr) {
        match expr {
            ast::Expr::QuantifiedExpr(q) => {
                if let Some(inner) = q.inner() {
                    let captures = self.collect_propagating_captures(&inner);
                    if captures.len() >= 2 {
                        self.qis_triggers.insert(q.clone(), QisTrigger { captures });
                    }
                    self.detect_qis_in_expr(&inner);
                }
            }
            ast::Expr::CapturedExpr(c) => {
                // Captures on sequences/alternations absorb inner captures,
                // but we still recurse to find nested quantifiers
                if let Some(inner) = c.inner() {
                    self.detect_qis_in_expr(&inner);
                }
            }
            _ => {
                for child in expr.children() {
                    self.detect_qis_in_expr(&child);
                }
            }
        }
    }

    /// Collect captures that propagate out of an expression (not absorbed by inner scopes).
    fn collect_propagating_captures(&self, expr: &ast::Expr) -> Vec<&'a str> {
        let mut captures = Vec::new();
        self.collect_propagating_captures_impl(expr, &mut captures);
        captures
    }

    fn collect_propagating_captures_impl(&self, expr: &ast::Expr, out: &mut Vec<&'a str>) {
        match expr {
            ast::Expr::CapturedExpr(c) => {
                if let Some(name_token) = c.name() {
                    let name = token_src(&name_token, self.source);
                    out.push(name);
                }
                // Captured sequence/alternation absorbs inner captures.
                // Need to look through quantifiers to find the actual container.
                if let Some(inner) = c.inner() {
                    if !Self::is_scope_container(&inner) {
                        self.collect_propagating_captures_impl(&inner, out);
                    }
                }
            }
            ast::Expr::QuantifiedExpr(q) => {
                // Nested quantifier: its captures propagate (with modified cardinality)
                if let Some(inner) = q.inner() {
                    self.collect_propagating_captures_impl(&inner, out);
                }
            }
            _ => {
                for child in expr.children() {
                    self.collect_propagating_captures_impl(&child, out);
                }
            }
        }
    }

    /// Check if an expression is a scope container (seq/alt), looking through quantifiers.
    fn is_scope_container(expr: &ast::Expr) -> bool {
        match expr {
            ast::Expr::SeqExpr(_) | ast::Expr::AltExpr(_) => true,
            ast::Expr::QuantifiedExpr(q) => q
                .inner()
                .map(|i| Self::is_scope_container(&i))
                .unwrap_or(false),
            _ => false,
        }
    }

    /// Check if a quantified expression triggers QIS.
    pub fn is_qis_trigger(&self, q: &ast::QuantifiedExpr) -> bool {
        self.qis_triggers.contains_key(q)
    }

    /// Get QIS trigger info for a quantified expression.
    pub fn qis_trigger(&self, q: &ast::QuantifiedExpr) -> Option<&QisTrigger<'a>> {
        self.qis_triggers.get(q)
    }
}
