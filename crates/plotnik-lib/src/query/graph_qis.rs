//! Capture scope detection: QIS and single-capture definitions.
//!
//! - QIS triggers when a quantified expression has ≥2 propagating captures.
//! - Single-capture definitions unwrap to their capture's type directly.
//!
//! See ADR-0009 for full specification.

use std::collections::{HashMap, HashSet};

use crate::parser::{ast, token_src};
use crate::query::symbol_table::SymbolTable;
use crate::query::visitor::Visitor;

#[derive(Debug, Clone)]
pub struct QisTrigger<'a> {
    #[allow(unused)]
    pub captures: Vec<&'a str>,
}

pub type QisTriggerTable<'q> = HashMap<ast::QuantifiedExpr, QisTrigger<'q>>;

#[derive(Debug, Default)]
pub struct QisContext<'q> {
    pub qis_triggers: QisTriggerTable<'q>,
    /// Definitions with exactly 1 propagating capture: def name → capture name.
    pub single_capture_defs: HashMap<&'q str, &'q str>,
    /// Definitions with 2+ propagating captures (need struct wrapping at root).
    pub multi_capture_defs: HashSet<&'q str>,
}

/// Detect capture scopes: QIS triggers and single-capture definitions.
///
/// - QIS triggers when quantified expression has ≥2 propagating captures
/// - Single-capture definitions unwrap (no Field effect, type is capture's type)
pub fn detect_capture_scopes<'q>(
    source: &'q str,
    symbol_table: &SymbolTable<'q>,
) -> QisContext<'q> {
    let mut ctx: QisContext<'q> = QisContext::default();

    let mut visitor = QisVisitor {
        source,
        qis_triggers: &mut ctx.qis_triggers,
    };

    // Collect entries to decouple from self for the iteration
    let entries: Vec<_> = symbol_table.iter().map(|(n, b)| (*n, b.clone())).collect();

    for (name, body) in entries {
        // 1. Detect single/multi capture definitions
        let captures = collect_propagating_captures(&body, source);

        if captures.len() == 1 {
            ctx.single_capture_defs.insert(name, captures[0]);
        } else if captures.len() >= 2 {
            ctx.multi_capture_defs.insert(name);
        }

        // 2. Detect QIS within this definition
        visitor.visit_expr(&body);
    }

    ctx
}

struct QisVisitor<'a, 'map> {
    source: &'a str,
    qis_triggers: &'map mut HashMap<ast::QuantifiedExpr, QisTrigger<'a>>,
}

impl<'a, 'map> Visitor for QisVisitor<'a, 'map> {
    fn visit_quantified_expr(&mut self, q: &ast::QuantifiedExpr) {
        if let Some(inner) = q.inner() {
            let captures = collect_propagating_captures(&inner, self.source);
            if captures.len() >= 2 {
                self.qis_triggers.insert(q.clone(), QisTrigger { captures });
            }
            // Recurse
            self.visit_expr(&inner);
        }
    }

    fn visit_captured_expr(&mut self, c: &ast::CapturedExpr) {
        // Captures on sequences/alternations absorb inner captures,
        // but we still recurse to find nested quantifiers.
        if let Some(inner) = c.inner() {
            // Special case: captured quantifier with ≥1 nested capture needs QIS
            // to wrap each iteration with StartObject/EndObject for proper field scoping.
            if let ast::Expr::QuantifiedExpr(q) = &inner
                && let Some(quant_inner) = q.inner()
            {
                let captures = collect_propagating_captures(&quant_inner, self.source);
                // Trigger QIS if there's at least 1 capture (not already covered by ≥2 rule)
                if !captures.is_empty() && !self.qis_triggers.contains_key(q) {
                    self.qis_triggers.insert(q.clone(), QisTrigger { captures });
                }
            }
            self.visit_expr(&inner);
        }
    }
}

pub fn collect_propagating_captures<'a>(expr: &ast::Expr, source: &'a str) -> Vec<&'a str> {
    let mut collector = CaptureCollector {
        source,
        captures: Vec::new(),
    };
    collector.visit_expr(expr);
    collector.captures
}

struct CaptureCollector<'a> {
    source: &'a str,
    captures: Vec<&'a str>,
}

impl<'a> Visitor for CaptureCollector<'a> {
    fn visit_captured_expr(&mut self, c: &ast::CapturedExpr) {
        if let Some(name_token) = c.name() {
            let name = token_src(&name_token, self.source);
            self.captures.push(name);
        }

        // Captured sequence/alternation absorbs inner captures.
        // Captured quantifiers with nested captures also absorb (they become QIS).
        if let Some(inner) = c.inner()
            && !is_scope_container(&inner, self.source)
        {
            self.visit_expr(&inner);
        }
    }

    fn visit_quantified_expr(&mut self, q: &ast::QuantifiedExpr) {
        // Nested quantifier: its captures propagate (with modified cardinality)
        if let Some(inner) = q.inner() {
            self.visit_expr(&inner);
        }
    }
}

/// Check if an expression is a scope container that absorbs inner captures.
/// - Sequences and alternations always absorb
/// - Quantifiers absorb if they have nested captures (will become QIS)
fn is_scope_container(expr: &ast::Expr, source: &str) -> bool {
    match expr {
        ast::Expr::SeqExpr(_) | ast::Expr::AltExpr(_) => true,
        ast::Expr::QuantifiedExpr(q) => {
            if let Some(inner) = q.inner() {
                // Quantifier with nested captures acts as scope container
                // (will be treated as QIS, wrapping each element in an object)
                let nested_captures = collect_propagating_captures(&inner, source);
                if !nested_captures.is_empty() {
                    return true;
                }
                // Otherwise check if inner is a scope container
                is_scope_container(&inner, source)
            } else {
                false
            }
        }
        _ => false,
    }
}
