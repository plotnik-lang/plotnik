//! Symbol table: name resolution and reference checking.
//!
//! Two-pass approach:
//! 1. Collect all `Name = expr` definitions from all sources
//! 2. Check that all `(UpperIdent)` references are defined

use indexmap::IndexMap;

use crate::Diagnostics;
use crate::diagnostics::DiagnosticKind;
use crate::parser::{Root, ast, token_src};

use super::source_map::{SourceId, SourceMap};
use super::visitor::Visitor;

/// Sentinel name for unnamed definitions (bare expressions at root level).
/// Code generators can emit whatever name they want for this.
pub const UNNAMED_DEF: &str = "_";

pub type SymbolTable<'src> = IndexMap<&'src str, (SourceId, ast::Expr)>;

pub fn resolve_names<'q>(
    source_map: &'q SourceMap,
    ast_map: &IndexMap<SourceId, Root>,
    diag: &mut Diagnostics,
) -> SymbolTable<'q> {
    let mut symbol_table = SymbolTable::default();

    // Pass 1: collect definitions from all sources
    for (&source_id, ast) in ast_map {
        let src = source_map.content(source_id);
        let mut resolver = ReferenceResolver {
            src,
            source_id,
            diag,
            symbol_table: &mut symbol_table,
        };
        resolver.visit(ast);
    }

    // Pass 2: validate references from all sources
    for (&source_id, ast) in ast_map {
        let src = source_map.content(source_id);
        let mut validator = ReferenceValidator {
            src,
            diag,
            symbol_table: &symbol_table,
        };
        validator.visit(ast);
    }

    symbol_table
}

struct ReferenceResolver<'q, 'd, 't> {
    src: &'q str,
    source_id: SourceId,
    diag: &'d mut Diagnostics,
    symbol_table: &'t mut SymbolTable<'q>,
}

impl Visitor for ReferenceResolver<'_, '_, '_> {
    fn visit_def(&mut self, def: &ast::Def) {
        let Some(body) = def.body() else { return };

        if let Some(token) = def.name() {
            // Named definition: `Name = ...`
            let name = token_src(&token, self.src);
            if self.symbol_table.contains_key(name) {
                self.diag
                    .report(DiagnosticKind::DuplicateDefinition, token.text_range())
                    .message(name)
                    .emit();
            } else {
                self.symbol_table.insert(name, (self.source_id, body));
            }
        } else {
            // Unnamed definition: `...` (root expression)
            // Parser already validates multiple unnamed defs; we keep the last one.
            if self.symbol_table.contains_key(UNNAMED_DEF) {
                self.symbol_table.shift_remove(UNNAMED_DEF);
            }
            self.symbol_table
                .insert(UNNAMED_DEF, (self.source_id, body));
        }
    }
}

struct ReferenceValidator<'q, 'd, 't> {
    #[allow(dead_code)]
    src: &'q str,
    diag: &'d mut Diagnostics,
    symbol_table: &'t SymbolTable<'q>,
}

impl Visitor for ReferenceValidator<'_, '_, '_> {
    fn visit_ref(&mut self, r: &ast::Ref) {
        let Some(name_token) = r.name() else { return };
        let name = name_token.text();

        if self.symbol_table.contains_key(name) {
            return;
        }

        self.diag
            .report(DiagnosticKind::UndefinedReference, name_token.text_range())
            .message(name)
            .emit();
    }
}
