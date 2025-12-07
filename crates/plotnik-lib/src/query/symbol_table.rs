//! Symbol table: name resolution and reference checking.
//!
//! Two-pass approach:
//! 1. Collect all `Name = expr` definitions
//! 2. Check that all `(UpperIdent)` references are defined

use indexmap::IndexMap;

use crate::diagnostics::DiagnosticKind;
use crate::parser::{Expr, Ref, ast, token_src};

use super::Query;

pub type SymbolTable<'src> = IndexMap<&'src str, ast::Expr>;

impl<'a> Query<'a> {
    pub(super) fn resolve_names(&mut self) {
        // Pass 1: collect definitions
        for def in self.ast.defs() {
            let Some(name_token) = def.name() else {
                continue;
            };

            let name = token_src(&name_token, self.source);

            if self.symbol_table.contains_key(name) {
                self.resolve_diagnostics
                    .report(DiagnosticKind::DuplicateDefinition, name_token.text_range())
                    .message(name)
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
        if let Expr::Ref(r) = expr {
            self.check_ref_diagnostic(r);
        }

        for child in expr.children() {
            self.collect_reference_diagnostics(&child);
        }
    }

    fn check_ref_diagnostic(&mut self, r: &Ref) {
        let Some(name_token) = r.name() else { return };
        let name = name_token.text();

        if self.symbol_table.contains_key(name) {
            return;
        }

        self.resolve_diagnostics
            .report(DiagnosticKind::UndefinedReference, name_token.text_range())
            .message(name)
            .emit();
    }
}
