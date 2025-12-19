//! Symbol table: name resolution and reference checking.
//!
//! Two-pass approach:
//! 1. Collect all `Name = expr` definitions
//! 2. Check that all `(UpperIdent)` references are defined

use indexmap::IndexMap;

/// Sentinel name for unnamed definitions (bare expressions at root level).
/// Code generators can emit whatever name they want for this.
pub const UNNAMED_DEF: &str = "_";

use crate::Diagnostics;
use crate::diagnostics::DiagnosticKind;
use crate::parser::{Root, ast, token_src};

use super::visitor::Visitor;

pub type SymbolTable<'src> = IndexMap<&'src str, ast::Expr>;

pub fn resolve_names<'q>(ast: &Root, src: &'q str, diag: &mut Diagnostics) -> SymbolTable<'q> {
    let symbol_table = SymbolTable::default();
    let ctx = Context {
        src,
        diag,
        symbol_table,
    };

    let mut resolver = ReferenceResolver { ctx };
    resolver.visit(ast);
    let ctx = resolver.ctx;

    let mut validator = ReferenceValidator { ctx };
    validator.visit(ast);
    validator.ctx.symbol_table
}

struct Context<'q, 'd> {
    src: &'q str,
    diag: &'d mut Diagnostics,
    symbol_table: SymbolTable<'q>,
}

struct ReferenceResolver<'q, 'd> {
    pub ctx: Context<'q, 'd>,
}

impl Visitor for ReferenceResolver<'_, '_> {
    fn visit_def(&mut self, def: &ast::Def) {
        let Some(body) = def.body() else { return };

        if let Some(token) = def.name() {
            // Named definition: `Name = ...`
            let name = token_src(&token, self.ctx.src);
            if self.ctx.symbol_table.contains_key(name) {
                self.ctx
                    .diag
                    .report(DiagnosticKind::DuplicateDefinition, token.text_range())
                    .message(name)
                    .emit();
            } else {
                self.ctx.symbol_table.insert(name, body);
            }
        } else {
            // Unnamed definition: `...` (root expression)
            // Parser already validates multiple unnamed defs; we keep the last one.
            if self.ctx.symbol_table.contains_key(UNNAMED_DEF) {
                self.ctx.symbol_table.shift_remove(UNNAMED_DEF);
            }
            self.ctx.symbol_table.insert(UNNAMED_DEF, body);
        }
    }
}

struct ReferenceValidator<'q, 'd> {
    pub ctx: Context<'q, 'd>,
}

impl Visitor for ReferenceValidator<'_, '_> {
    fn visit_ref(&mut self, r: &ast::Ref) {
        let Some(name_token) = r.name() else { return };
        let name = name_token.text();

        if self.ctx.symbol_table.contains_key(name) {
            return;
        }

        self.ctx
            .diag
            .report(DiagnosticKind::UndefinedReference, name_token.text_range())
            .message(name)
            .emit();
    }
}
