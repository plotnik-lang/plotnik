//! Capture scope detection: QIS and single-capture definitions.
//!
//! - QIS triggers when a quantified expression has ≥2 propagating captures.
//! - Single-capture definitions unwrap to their capture's type directly.
//!
//! See ADR-0009 for full specification.

use crate::parser::{ast, token_src};

use super::{QisTrigger, Query};

impl<'a> Query<'a> {
    /// Detect capture scopes: QIS triggers and single-capture definitions.
    ///
    /// - QIS triggers when quantified expression has ≥2 propagating captures
    /// - Single-capture definitions unwrap (no Field effect, type is capture's type)
    pub(super) fn detect_capture_scopes(&mut self) {
        let entries: Vec<_> = self
            .symbol_table
            .iter()
            .map(|(n, b)| (*n, b.clone()))
            .collect();
        for (name, body) in &entries {
            // Detect single-capture and multi-capture definitions
            let captures = self.collect_propagating_captures(body);
            if captures.len() == 1 {
                self.single_capture_defs.insert(*name, captures[0]);
            } else if captures.len() >= 2 {
                self.multi_capture_defs.insert(*name);
            }
            // Detect QIS within this definition
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
                // but we still recurse to find nested quantifiers.
                // Special case: captured quantifier with ≥1 nested capture needs QIS
                // to wrap each iteration with StartObject/EndObject for proper field scoping.
                if let Some(inner) = c.inner() {
                    // Check if this capture wraps a quantifier with nested captures
                    if let ast::Expr::QuantifiedExpr(q) = &inner {
                        if let Some(quant_inner) = q.inner() {
                            let captures = self.collect_propagating_captures(&quant_inner);
                            // Trigger QIS if there's at least 1 capture (not already covered by ≥2 rule)
                            if !captures.is_empty() && !self.qis_triggers.contains_key(q) {
                                self.qis_triggers.insert(q.clone(), QisTrigger { captures });
                            }
                        }
                    }
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
    pub(super) fn collect_propagating_captures(&self, expr: &ast::Expr) -> Vec<&'a str> {
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
                // Captured quantifiers with nested captures also absorb (they become QIS).
                if let Some(inner) = c.inner()
                    && !self.is_scope_container(&inner)
                {
                    self.collect_propagating_captures_impl(&inner, out);
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

    /// Check if an expression is a scope container that absorbs inner captures.
    /// - Sequences and alternations always absorb
    /// - Quantifiers absorb if they have nested captures (will become QIS)
    fn is_scope_container(&self, expr: &ast::Expr) -> bool {
        match expr {
            ast::Expr::SeqExpr(_) | ast::Expr::AltExpr(_) => true,
            ast::Expr::QuantifiedExpr(q) => {
                if let Some(inner) = q.inner() {
                    // Quantifier with nested captures acts as scope container
                    // (will be treated as QIS, wrapping each element in an object)
                    let nested_captures = self.collect_propagating_captures(&inner);
                    if !nested_captures.is_empty() {
                        return true;
                    }
                    // Otherwise check if inner is a scope container
                    self.is_scope_container(&inner)
                } else {
                    false
                }
            }
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

    /// Check if this capture is the single propagating capture for its definition.
    /// Only that specific capture should unwrap (skip Field effect).
    pub fn is_single_capture(&self, def_name: &str, capture_name: &str) -> bool {
        self.single_capture_defs
            .get(def_name)
            .map(|c| *c == capture_name)
            .unwrap_or(false)
    }

    /// Check if definition has 2+ propagating captures (needs struct wrapping).
    pub fn is_multi_capture_def(&self, name: &str) -> bool {
        self.multi_capture_defs.contains(name)
    }
}
