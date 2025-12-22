//! Expression arity analysis for query expressions.
//!
//! Determines whether an expression matches a single node position (`One`)
//! or multiple sequential positions (`Many`). Used to validate field constraints:
//! `field: expr` requires `expr` to have `ExprArity::One`.
//!
//! `Invalid` marks nodes where arity cannot be determined (error nodes,
//! undefined refs, etc.).

use std::collections::HashMap;

use super::query::AstMap;
use super::source_map::SourceId;
use super::symbol_table::SymbolTable;
use super::visitor::{Visitor, walk_expr, walk_field_expr};
use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::parser::{Expr, FieldExpr, Ref, SeqExpr, SyntaxKind, SyntaxNode, ast};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExprArity {
    One,
    Many,
    Invalid,
}

pub type ExprArityTable = HashMap<Expr, ExprArity>;

pub fn infer_arities(
    ast_map: &AstMap,
    symbol_table: &SymbolTable,
    diag: &mut Diagnostics,
) -> ExprArityTable {
    let mut arity_table = ExprArityTable::default();

    for (&source_id, root) in ast_map {
        let ctx = ArityContext {
            symbol_table,
            arity_table,
            diag,
            source_id,
        };
        let mut computer = ArityComputer { ctx };
        computer.visit(root);
        arity_table = computer.ctx.arity_table;
    }

    for (&source_id, root) in ast_map {
        let ctx = ArityContext {
            symbol_table,
            arity_table,
            diag,
            source_id,
        };
        let mut validator = ArityValidator { ctx };
        validator.visit(root);
        arity_table = validator.ctx.arity_table;
    }

    arity_table
}

pub fn resolve_arity(node: &SyntaxNode, table: &ExprArityTable) -> Option<ExprArity> {
    if node.kind() == SyntaxKind::Error {
        return Some(ExprArity::Invalid);
    }

    // Try casting to Expr first as it's the most common query
    if let Some(expr) = ast::Expr::cast(node.clone()) {
        return table.get(&expr).copied();
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
        return def.body().and_then(|b| table.get(&b).copied());
    }

    // Branch: delegate to body's arity
    if let Some(branch) = ast::Branch::cast(node.clone()) {
        return branch.body().and_then(|b| table.get(&b).copied());
    }

    None
}

struct ArityContext<'a, 'd> {
    symbol_table: &'a SymbolTable,
    arity_table: ExprArityTable,
    diag: &'d mut Diagnostics,
    source_id: SourceId,
}

impl ArityContext<'_, '_> {
    fn compute_arity(&mut self, expr: &Expr) -> ExprArity {
        if let Some(&c) = self.arity_table.get(expr) {
            return c;
        }
        // Insert sentinel to break cycles (e.g., `Foo = (Foo)`)
        self.arity_table.insert(expr.clone(), ExprArity::Invalid);

        let c = self.compute_single_arity(expr);
        self.arity_table.insert(expr.clone(), c);
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
            .map(|body| self.compute_arity(body))
            .unwrap_or(ExprArity::Invalid)
    }

    fn validate_field(&mut self, field: &FieldExpr) {
        let Some(value) = field.value() else {
            return;
        };

        let card = self
            .arity_table
            .get(&value)
            .copied()
            .unwrap_or(ExprArity::One);

        if card == ExprArity::Many {
            let field_name = field
                .name()
                .map(|t| t.text().to_string())
                .unwrap_or_else(|| "field".to_string());

            let mut builder = self
                .diag
                .report(
                    self.source_id,
                    DiagnosticKind::FieldSequenceValue,
                    value.text_range(),
                )
                .message(field_name);

            // If value is a reference, add related info pointing to definition
            if let Expr::Ref(r) = &value
                && let Some(name_tok) = r.name()
                && let Some((def_source, def_body)) = self.symbol_table.get_full(name_tok.text())
            {
                builder = builder.related_to(def_source, def_body.text_range(), "defined here");
            }

            builder.emit();
        }
    }
}

struct ArityComputer<'a, 'd> {
    ctx: ArityContext<'a, 'd>,
}

impl Visitor for ArityComputer<'_, '_> {
    fn visit_expr(&mut self, expr: &Expr) {
        self.ctx.compute_arity(expr);
        walk_expr(self, expr);
    }
}

struct ArityValidator<'a, 'd> {
    ctx: ArityContext<'a, 'd>,
}

impl Visitor for ArityValidator<'_, '_> {
    fn visit_field_expr(&mut self, field: &FieldExpr) {
        self.ctx.validate_field(field);
        walk_field_expr(self, field);
    }
}
