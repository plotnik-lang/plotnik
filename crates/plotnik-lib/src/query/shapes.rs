//! Shape cardinality analysis for query expressions.
//!
//! Determines whether an expression matches a single node position (`One`)
//! or multiple sequential positions (`Many`). Used to validate field constraints:
//! `field: expr` requires `expr` to have `ShapeCardinality::One`.
//!
//! `Invalid` marks nodes where cardinality cannot be determined (error nodes,
//! undefined refs, etc.).

use super::Query;
use super::invariants::ensure_ref_has_name;
use crate::diagnostics::DiagnosticKind;
use crate::parser::{Expr, FieldExpr, Ref, SeqExpr, SyntaxNode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShapeCardinality {
    One,
    Many,
    Invalid,
}

impl Query<'_> {
    pub(super) fn infer_shapes(&mut self) {
        self.compute_all_cardinalities(self.ast.as_cst().clone());
        self.validate_shapes(self.ast.as_cst().clone());
    }

    fn compute_all_cardinalities(&mut self, node: SyntaxNode) {
        if let Some(expr) = Expr::cast(node.clone()) {
            self.get_or_compute(&expr);
        }

        for child in node.children() {
            self.compute_all_cardinalities(child);
        }
    }

    fn get_or_compute(&mut self, expr: &Expr) -> ShapeCardinality {
        if let Some(&c) = self.shape_cardinality_table.get(expr) {
            return c;
        }
        // Insert sentinel to break cycles (e.g., `Foo = (Foo)`)
        self.shape_cardinality_table
            .insert(expr.clone(), ShapeCardinality::Invalid);
        let c = self.compute_single(expr);
        self.shape_cardinality_table.insert(expr.clone(), c);
        c
    }

    fn compute_single(&mut self, expr: &Expr) -> ShapeCardinality {
        match expr {
            Expr::NamedNode(_) | Expr::AnonymousNode(_) | Expr::FieldExpr(_) | Expr::AltExpr(_) => {
                ShapeCardinality::One
            }

            Expr::SeqExpr(seq) => self.seq_cardinality(seq),

            Expr::CapturedExpr(cap) => {
                let Some(inner) = cap.inner() else {
                    return ShapeCardinality::Invalid;
                };
                self.get_or_compute(&inner)
            }

            Expr::QuantifiedExpr(q) => {
                let Some(inner) = q.inner() else {
                    return ShapeCardinality::Invalid;
                };
                self.get_or_compute(&inner)
            }

            Expr::Ref(r) => self.ref_cardinality(r),
        }
    }

    fn seq_cardinality(&mut self, seq: &SeqExpr) -> ShapeCardinality {
        let children: Vec<_> = seq.children().collect();

        match children.len() {
            0 => ShapeCardinality::One,
            1 => self.get_or_compute(&children[0]),
            _ => ShapeCardinality::Many,
        }
    }

    fn ref_cardinality(&mut self, r: &Ref) -> ShapeCardinality {
        let name_tok = ensure_ref_has_name(r.name());
        let name = name_tok.text();

        let Some(body) = self.symbol_table.get(name).cloned() else {
            return ShapeCardinality::Invalid;
        };

        self.get_or_compute(&body)
    }

    fn validate_shapes(&mut self, node: SyntaxNode) {
        let Some(field) = FieldExpr::cast(node.clone()) else {
            for child in node.children() {
                self.validate_shapes(child);
            }
            return;
        };

        let Some(value) = field.value() else {
            for child in node.children() {
                self.validate_shapes(child);
            }
            return;
        };

        let card = self
            .shape_cardinality_table
            .get(&value)
            .copied()
            .unwrap_or(ShapeCardinality::One);

        if card == ShapeCardinality::Many {
            let field_name = field
                .name()
                .map(|t| t.text().to_string())
                .unwrap_or_else(|| "field".to_string());

            self.shapes_diagnostics
                .report(DiagnosticKind::FieldSequenceValue, value.text_range())
                .message(field_name)
                .emit();
        }

        for child in node.children() {
            self.validate_shapes(child);
        }
    }
}
