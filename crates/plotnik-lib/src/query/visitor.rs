//! AST Visitor pattern.
//!
//! # Usage
//!
//! Implement `Visitor` for your struct. Override `visit_*` methods to add logic.
//! Call `walk_*` within your override to continue recursion (or omit it to stop).
//!
//! ```ignore
//! impl Visitor for MyPass {
//!     fn visit_named_node(&mut self, node: &NamedNode) {
//!         // Pre-order logic
//!         walk_named_node(self, node);
//!         // Post-order logic
//!     }
//! }
//! ```

use crate::parser::ast::{
    AltExpr, AnonymousNode, CapturedExpr, Def, Expr, FieldExpr, NamedNode, QuantifiedExpr, Ref,
    Root, SeqExpr,
};

pub trait Visitor: Sized {
    fn visit_root(&mut self, root: &Root) {
        walk_root(self, root);
    }

    fn visit_def(&mut self, def: &Def) {
        walk_def(self, def);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        walk_expr(self, expr);
    }

    fn visit_named_node(&mut self, node: &NamedNode) {
        walk_named_node(self, node);
    }

    fn visit_anonymous_node(&mut self, _node: &AnonymousNode) {
        // Leaf node
    }

    fn visit_ref(&mut self, _ref: &Ref) {
        // Leaf node in AST structure (semantic traversal happens via SymbolTable lookup)
    }

    fn visit_alt_expr(&mut self, alt: &AltExpr) {
        walk_alt_expr(self, alt);
    }

    fn visit_seq_expr(&mut self, seq: &SeqExpr) {
        walk_seq_expr(self, seq);
    }

    fn visit_captured_expr(&mut self, cap: &CapturedExpr) {
        walk_captured_expr(self, cap);
    }

    fn visit_quantified_expr(&mut self, quant: &QuantifiedExpr) {
        walk_quantified_expr(self, quant);
    }

    fn visit_field_expr(&mut self, field: &FieldExpr) {
        walk_field_expr(self, field);
    }
}

pub fn walk_root<V: Visitor>(visitor: &mut V, root: &Root) {
    for def in root.defs() {
        visitor.visit_def(&def);
    }
}

pub fn walk_def<V: Visitor>(visitor: &mut V, def: &Def) {
    if let Some(body) = def.body() {
        visitor.visit_expr(&body);
    }
}

pub fn walk_expr<V: Visitor>(visitor: &mut V, expr: &Expr) {
    match expr {
        Expr::NamedNode(n) => visitor.visit_named_node(n),
        Expr::AnonymousNode(n) => visitor.visit_anonymous_node(n),
        Expr::Ref(r) => visitor.visit_ref(r),
        Expr::AltExpr(a) => visitor.visit_alt_expr(a),
        Expr::SeqExpr(s) => visitor.visit_seq_expr(s),
        Expr::CapturedExpr(c) => visitor.visit_captured_expr(c),
        Expr::QuantifiedExpr(q) => visitor.visit_quantified_expr(q),
        Expr::FieldExpr(f) => visitor.visit_field_expr(f),
    }
}

pub fn walk_named_node<V: Visitor>(visitor: &mut V, node: &NamedNode) {
    // We iterate specific children to avoid Expr::children() Vec allocation
    for child in node.children() {
        visitor.visit_expr(&child);
    }
}

pub fn walk_alt_expr<V: Visitor>(visitor: &mut V, alt: &AltExpr) {
    for branch in alt.branches() {
        if let Some(body) = branch.body() {
            visitor.visit_expr(&body);
        }
    }
    // Also visit bare exprs in untagged/mixed alts if any exist unwrapped
    for expr in alt.exprs() {
        visitor.visit_expr(&expr);
    }
}

pub fn walk_seq_expr<V: Visitor>(visitor: &mut V, seq: &SeqExpr) {
    for child in seq.children() {
        visitor.visit_expr(&child);
    }
}

pub fn walk_captured_expr<V: Visitor>(visitor: &mut V, cap: &CapturedExpr) {
    if let Some(inner) = cap.inner() {
        visitor.visit_expr(&inner);
    }
}

pub fn walk_quantified_expr<V: Visitor>(visitor: &mut V, quant: &QuantifiedExpr) {
    if let Some(inner) = quant.inner() {
        visitor.visit_expr(&inner);
    }
}

pub fn walk_field_expr<V: Visitor>(visitor: &mut V, field: &FieldExpr) {
    if let Some(val) = field.value() {
        visitor.visit_expr(&val);
    }
}
