//! Symbol table: name resolution and reference checking.
//!
//! Two-pass approach:
//! 1. Collect all `Name = expr` definitions
//! 2. Check that all `(UpperIdent)` references are defined

use indexmap::IndexMap;

use crate::parser::{Expr, Ref, ast};

use super::Query;

pub type SymbolTable<'src> = IndexMap<&'src str, ast::Expr>;

impl<'a> Query<'a> {
    pub(super) fn resolve_names(&mut self) {
        // Pass 1: collect definitions
        for def in self.ast.defs() {
            let Some(name_token) = def.name() else {
                continue;
            };

            let range = name_token.text_range();
            let name = &self.source[range.start().into()..range.end().into()];

            if self.symbol_table.contains_key(name) {
                self.resolve_diagnostics
                    .error(format!("duplicate definition: `{}`", name), range)
                    .emit();
                continue;
            }

            let Some(body) = def.body() else {
                continue;
            };
            self.symbol_table.insert(name, body);
        }

        // Pass 2: check references
        let defs: Vec<_> = self.ast.defs().collect();
        for def in defs {
            let Some(body) = def.body() else { continue };
            self.collect_reference_diagnostics(&body);
        }

        // Parser wraps all top-level exprs in Def nodes, so this should be empty
        assert!(
            self.ast.exprs().next().is_none(),
            "symbol_table: unexpected bare Expr in Root (parser should wrap in Def)"
        );
    }

    fn collect_reference_diagnostics(&mut self, expr: &Expr) {
        match expr {
            Expr::Ref(r) => {
                self.check_ref_diagnostic(r);
            }
            Expr::NamedNode(node) => {
                for child in node.children() {
                    self.collect_reference_diagnostics(&child);
                }
            }
            Expr::AltExpr(alt) => {
                for branch in alt.branches() {
                    let Some(body) = branch.body() else { continue };
                    self.collect_reference_diagnostics(&body);
                }
                // Parser wraps all alt children in Branch nodes
                assert!(
                    alt.exprs().next().is_none(),
                    "symbol_table: unexpected bare Expr in Alt (parser should wrap in Branch)"
                );
            }
            Expr::SeqExpr(seq) => {
                for child in seq.children() {
                    self.collect_reference_diagnostics(&child);
                }
            }
            Expr::CapturedExpr(cap) => {
                let Some(inner) = cap.inner() else { return };
                self.collect_reference_diagnostics(&inner);
            }
            Expr::QuantifiedExpr(q) => {
                let Some(inner) = q.inner() else { return };
                self.collect_reference_diagnostics(&inner);
            }
            Expr::FieldExpr(f) => {
                let Some(value) = f.value() else { return };
                self.collect_reference_diagnostics(&value);
            }
            Expr::AnonymousNode(_) => {}
        }
    }

    fn check_ref_diagnostic(&mut self, r: &Ref) {
        let Some(name_token) = r.name() else { return };
        let name = name_token.text();

        if self.symbol_table.contains_key(name) {
            return;
        }

        self.resolve_diagnostics
            .error(
                format!("undefined reference: `{}`", name),
                name_token.text_range(),
            )
            .emit();
    }
}
