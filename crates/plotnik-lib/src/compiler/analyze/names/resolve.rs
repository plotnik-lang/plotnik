//! Name-resolution pass: build the symbol table, then check references.
//!
//! Two passes over every source:
//! 1. Collect all `Name = pattern` definitions into a [`SymbolTable`].
//! 2. Check that every `(UpperIdent)` reference resolves to a definition.

use crate::compiler::analyze::Located;
use crate::compiler::analyze::shape::validation::ValidatedAst;
use crate::compiler::analyze::visitor::Visitor;
use crate::compiler::diagnostics::diagnostics::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::{self, token_src};

use super::symbol_table::{SymbolTable, SymbolTableBuilder};

pub fn resolve_names(validated: &ValidatedAst<'_>, diag: &mut Diagnostics) -> SymbolTable {
    let mut builder = SymbolTableBuilder::new();

    for (&source_id, ast) in validated.ast_map() {
        let src = validated.source_map().content(source_id);
        let mut resolver = ReferenceResolver {
            src,
            diag: &mut *diag,
            builder: &mut builder,
        };
        resolver.visit(&Located::new(source_id, ast.clone()));
    }

    let symbol_table = builder.finish();

    for (&source_id, ast) in validated.ast_map() {
        let mut validator = ReferenceValidator {
            diag: &mut *diag,
            symbol_table: &symbol_table,
        };
        validator.visit(&Located::new(source_id, ast.clone()));
    }

    symbol_table
}

struct ReferenceResolver<'q, 'd, 'a> {
    src: &'q str,
    diag: &'d mut Diagnostics,
    builder: &'a mut SymbolTableBuilder,
}

impl Visitor for ReferenceResolver<'_, '_, '_> {
    fn visit_def(&mut self, def: &Located<ast::Def>) {
        let Some(body) = def.node().body() else {
            return;
        };
        // A nameless def is a parser-level error (MissingDefName); there is no name
        // to resolve, so it never enters the table.
        let Some(token) = def.node().name() else {
            return;
        };

        let name = token_src(&token, self.src);
        if self.builder.contains(name) {
            self.diag
                .report(
                    DiagnosticKind::DuplicateDefinition,
                    Span::new(def.source(), token.text_range()),
                )
                .detail(name)
                .emit();
        } else {
            self.builder.insert(name, def.source(), body);
        }
    }
}

struct ReferenceValidator<'d, 'a> {
    diag: &'d mut Diagnostics,
    symbol_table: &'a SymbolTable,
}

impl Visitor for ReferenceValidator<'_, '_> {
    fn visit_ref(&mut self, r: &Located<ast::Ref>) {
        let Some(name_token) = r.node().name() else {
            return;
        };
        let name = name_token.text();

        if self.symbol_table.defined_name(name).is_some() {
            return;
        }

        self.diag
            .report(
                DiagnosticKind::UndefinedReference,
                Span::new(r.source(), name_token.text_range()),
            )
            .detail(name)
            .emit();
    }
}
