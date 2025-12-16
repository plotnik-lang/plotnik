//! Expression arity analysis for query expressions.
//!
//! Determines whether an expression matches a single node position (`One`)
//! or multiple sequential positions (`Many`). Used to validate field constraints:
//! `field: expr` requires `expr` to have `ExprArity::One`.
//!
//! `Invalid` marks nodes where cardinality cannot be determined (error nodes,
//! undefined refs, etc.).

use super::Query;
use super::visitor::{Visitor, walk_expr, walk_field_expr};
use crate::diagnostics::DiagnosticKind;
use crate::parser::{Expr, FieldExpr, Ref, SeqExpr, SyntaxKind, SyntaxNode, ast};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExprArity {
    One,
    Many,
    Invalid,
}

impl Query<'_> {
    pub(super) fn infer_arities(&mut self) {
        let root = self.ast.clone();

        let mut computer = ArityComputer { query: self };
        computer.visit_root(&root);

        let mut validator = ArityValidator { query: self };
        validator.visit_root(&root);
    }

    pub(super) fn shape_arity(&self, node: &SyntaxNode) -> ExprArity {
        // Error nodes are invalid
        if node.kind() == SyntaxKind::Error {
            return ExprArity::Invalid;
        }

        // Root: cardinality based on definition count
        if let Some(root) = ast::Root::cast(node.clone()) {
            return if root.defs().count() > 1 {
                ExprArity::Many
            } else {
                ExprArity::One
            };
        }

        // Def: delegate to body's cardinality
        if let Some(def) = ast::Def::cast(node.clone()) {
            return def
                .body()
                .and_then(|b| self.expr_arity_table.get(&b).copied())
                .unwrap_or(ExprArity::Invalid);
        }

        // Branch: delegate to body's cardinality
        if let Some(branch) = ast::Branch::cast(node.clone()) {
            return branch
                .body()
                .and_then(|b| self.expr_arity_table.get(&b).copied())
                .unwrap_or(ExprArity::Invalid);
        }

        // Expr: direct lookup
        ast::Expr::cast(node.clone())
            .and_then(|e| self.expr_arity_table.get(&e).copied())
            .unwrap_or(ExprArity::One)
    }
}

struct ArityComputer<'a, 'q> {
    query: &'a mut Query<'q>,
}

impl Visitor for ArityComputer<'_, '_> {
    fn visit_expr(&mut self, expr: &Expr) {
        self.query.compute_cardinality(expr);
        walk_expr(self, expr);
    }
}

struct ArityValidator<'a, 'q> {
    query: &'a mut Query<'q>,
}

impl Visitor for ArityValidator<'_, '_> {
    fn visit_field_expr(&mut self, field: &FieldExpr) {
        self.query.validate_field(field);
        walk_field_expr(self, field);
    }
}

impl Query<'_> {
    fn compute_cardinality(&mut self, expr: &Expr) -> ExprArity {
        if let Some(&c) = self.expr_arity_table.get(expr) {
            return c;
        }
        // Insert sentinel to break cycles (e.g., `Foo = (Foo)`)
        self.expr_arity_table
            .insert(expr.clone(), ExprArity::Invalid);
        let c = self.compute_single_cardinality(expr);
        self.expr_arity_table.insert(expr.clone(), c);
        c
    }

    fn compute_single_cardinality(&mut self, expr: &Expr) -> ExprArity {
        match expr {
            Expr::NamedNode(_) | Expr::AnonymousNode(_) | Expr::FieldExpr(_) | Expr::AltExpr(_) => {
                ExprArity::One
            }

            Expr::SeqExpr(seq) => self.seq_cardinality(seq),

            Expr::CapturedExpr(cap) => {
                let Some(inner) = cap.inner() else {
                    return ExprArity::Invalid;
                };
                self.compute_cardinality(&inner)
            }

            Expr::QuantifiedExpr(q) => {
                let Some(inner) = q.inner() else {
                    return ExprArity::Invalid;
                };
                self.compute_cardinality(&inner)
            }

            Expr::Ref(r) => self.ref_cardinality(r),
        }
    }

    fn seq_cardinality(&mut self, seq: &SeqExpr) -> ExprArity {
        let children: Vec<_> = seq.children().collect();

        match children.len() {
            0 => ExprArity::One,
            1 => self.compute_cardinality(&children[0]),
            _ => ExprArity::Many,
        }
    }

    fn ref_cardinality(&mut self, r: &Ref) -> ExprArity {
        let name_tok = r.name().expect(
            "expr_arities: Ref without name token \
             (parser only creates Ref for PascalCase Id)",
        );
        let name = name_tok.text();

        let Some(body) = self.symbol_table.get(name).cloned() else {
            return ExprArity::Invalid;
        };

        self.compute_cardinality(&body)
    }

    fn validate_field(&mut self, field: &FieldExpr) {
        let Some(value) = field.value() else {
            return;
        };

        let card = self
            .expr_arity_table
            .get(&value)
            .copied()
            .unwrap_or(ExprArity::One);

        if card == ExprArity::Many {
            let field_name = field
                .name()
                .map(|t| t.text().to_string())
                .unwrap_or_else(|| "field".to_string());

            self.expr_arity_diagnostics
                .report(DiagnosticKind::FieldSequenceValue, value.text_range())
                .message(field_name)
                .emit();
        }
    }
}
