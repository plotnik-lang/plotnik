//! Expression arity analysis for query expressions.
//!
//! Determines whether an expression matches a single node position (`One`)
//! or multiple sequential positions (`Many`). Used to validate field constraints:
//! `field: expr` requires `expr` to have `ExprArity::One`.
//!
//! `Invalid` marks nodes where arity cannot be determined (error nodes,
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
        computer.visit(&root);

        let mut validator = ArityValidator { query: self };
        validator.visit(&root);
    }

    pub(super) fn get_arity(&self, node: &SyntaxNode) -> Option<ExprArity> {
        if node.kind() == SyntaxKind::Error {
            return Some(ExprArity::Invalid);
        }

        // Try casting to Expr first as it's the most common query
        if let Some(expr) = ast::Expr::cast(node.clone()) {
            return self.expr_arity_table.get(&expr).copied();
        }

        // Root: arity based on definition count
        if let Some(root) = ast::Root::cast(node.clone()) {
            return Some(if root.defs().nth(1).is_some() {
                ExprArity::Many
            } else {
                ExprArity::One
            });
        }

        // Def: delegate to body's arity
        if let Some(def) = ast::Def::cast(node.clone()) {
            return def
                .body()
                .and_then(|b| self.expr_arity_table.get(&b).copied());
        }

        // Branch: delegate to body's arity
        if let Some(branch) = ast::Branch::cast(node.clone()) {
            return branch
                .body()
                .and_then(|b| self.expr_arity_table.get(&b).copied());
        }

        None
    }
}

struct ArityComputer<'a, 'q> {
    query: &'a mut Query<'q>,
}

impl Visitor for ArityComputer<'_, '_> {
    fn visit_expr(&mut self, expr: &Expr) {
        self.query.compute_arity(expr);
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
    fn compute_arity(&mut self, expr: &Expr) -> ExprArity {
        if let Some(&c) = self.expr_arity_table.get(expr) {
            return c;
        }
        // Insert sentinel to break cycles (e.g., `Foo = (Foo)`)
        self.expr_arity_table
            .insert(expr.clone(), ExprArity::Invalid);

        let c = self.compute_single_arity(expr);
        self.expr_arity_table.insert(expr.clone(), c);
        c
    }

    fn compute_single_arity(&mut self, expr: &Expr) -> ExprArity {
        match expr {
            Expr::NamedNode(_) | Expr::AnonymousNode(_) | Expr::FieldExpr(_) | Expr::AltExpr(_) => {
                ExprArity::One
            }

            Expr::SeqExpr(seq) => self.seq_arity(seq),

            Expr::CapturedExpr(cap) => cap
                .inner()
                .map(|inner| self.compute_arity(&inner))
                .unwrap_or(ExprArity::Invalid),

            Expr::QuantifiedExpr(q) => q
                .inner()
                .map(|inner| self.compute_arity(&inner))
                .unwrap_or(ExprArity::Invalid),

            Expr::Ref(r) => self.ref_arity(r),
        }
    }

    fn seq_arity(&mut self, seq: &SeqExpr) -> ExprArity {
        // Avoid collecting into Vec; check if we have 0, 1, or >1 children.
        let mut children = seq.children();

        match children.next() {
            None => ExprArity::One,
            Some(first) => {
                if children.next().is_some() {
                    ExprArity::Many
                } else {
                    self.compute_arity(&first)
                }
            }
        }
    }

    fn ref_arity(&mut self, r: &Ref) -> ExprArity {
        let name_tok = r.name().expect(
            "expr_arities: Ref without name token \
             (parser only creates Ref for PascalCase Id)",
        );
        let name = name_tok.text();

        self.symbol_table
            .get(name)
            .cloned()
            .map(|body| self.compute_arity(&body))
            .unwrap_or(ExprArity::Invalid)
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
