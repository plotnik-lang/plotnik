//! Symbol table: name resolution and reference checking.
//!
//! Two-pass approach:
//! 1. Collect all `Name = expr` definitions
//! 2. Check that all `(UpperIdent)` references are defined

use indexmap::IndexMap;

/// Sentinel name for unnamed definitions (bare expressions at root level).
/// Code generators can emit whatever name they want for this.
pub const UNNAMED_DEF: &str = "_";

use crate::diagnostics::DiagnosticKind;
use crate::parser::{ast, token_src};

use super::Query;
use super::visitor::{Visitor, walk_root};

pub type SymbolTable<'src> = IndexMap<&'src str, ast::Expr>;

impl<'a> Query<'a> {
    pub(super) fn resolve_names(&mut self) {
        // Pass 1: collect definitions
        for def in self.ast.defs() {
            let Some(body) = def.body() else { continue };

            if let Some(token) = def.name() {
                // Named definition: `Name = ...`
                let name = token_src(&token, self.source);
                if self.symbol_table.contains_key(name) {
                    self.resolve_diagnostics
                        .report(DiagnosticKind::DuplicateDefinition, token.text_range())
                        .message(name)
                        .emit();
                } else {
                    self.symbol_table.insert(name, body);
                }
            } else {
                // Unnamed definition: `...` (root expression)
                // Parser already validates multiple unnamed defs; we keep the last one.
                if self.symbol_table.contains_key(UNNAMED_DEF) {
                    self.symbol_table.shift_remove(UNNAMED_DEF);
                }
                self.symbol_table.insert(UNNAMED_DEF, body);
            }
        }

        // Pass 2: check references
        let root = self.ast.clone();
        let mut validator = ReferenceValidator { query: self };
        validator.visit_root(&root);
    }
}

struct ReferenceValidator<'a, 'q> {
    query: &'a mut Query<'q>,
}

impl Visitor for ReferenceValidator<'_, '_> {
    fn visit_root(&mut self, root: &ast::Root) {
        // Parser wraps all top-level exprs in Def nodes, so this should be empty
        assert!(
            root.exprs().next().is_none(),
            "symbol_table: unexpected bare Expr in Root (parser should wrap in Def)"
        );
        walk_root(self, root);
    }

    fn visit_ref(&mut self, r: &ast::Ref) {
        let Some(name_token) = r.name() else { return };
        let name = name_token.text();

        if self.query.symbol_table.contains_key(name) {
            return;
        }

        self.query
            .resolve_diagnostics
            .report(DiagnosticKind::UndefinedReference, name_token.text_range())
            .message(name)
            .emit();
    }
}
