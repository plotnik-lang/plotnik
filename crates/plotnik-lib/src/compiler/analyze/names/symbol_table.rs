//! Symbol table: name resolution and reference checking.
//!
//! Two-pass approach:
//! 1. Collect all `Name = pattern` definitions from all sources
//! 2. Check that all `(UpperIdent)` references are defined
//!
//! The `SymbolTable` registry itself lives in `compiler::core`; this module
//! owns the builder that fills it and the resolution/validation passes.

use indexmap::IndexMap;

use crate::compiler::core::{ast, token_src};
use crate::compiler::diagnostics::diagnostics::DiagnosticKind;
use crate::compiler::diagnostics::diagnostics::Diagnostics;

use crate::compiler::analyze::shape::validation::ValidatedAst;
use crate::compiler::analyze::Located;
use crate::compiler::core::source::SourceId;
use crate::compiler::analyze::visitor::Visitor;

pub use crate::compiler::core::SymbolTable;

/// Mutable accumulator for a [`SymbolTable`], owned by the name-resolution pass.
#[derive(Default)]
pub struct SymbolTableBuilder {
    table: IndexMap<String, ast::Pattern>,
    files: IndexMap<String, SourceId>,
}

impl SymbolTableBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.table.contains_key(name)
    }

    /// Record a definition. Returns `true` if newly inserted, `false` if it
    /// replaced an existing entry.
    pub fn insert(&mut self, name: &str, source_id: SourceId, pattern: ast::Pattern) -> bool {
        let is_new = !self.table.contains_key(name);
        self.table.insert(name.to_owned(), pattern);
        self.files.insert(name.to_owned(), source_id);
        is_new
    }

    /// Freeze the accumulated definitions into an immutable [`SymbolTable`].
    pub fn finish(self) -> SymbolTable {
        SymbolTable::new(self.table, self.files)
    }
}

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
                    def.source(),
                    DiagnosticKind::DuplicateDefinition,
                    token.text_range(),
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
                r.source(),
                DiagnosticKind::UndefinedReference,
                name_token.text_range(),
            )
            .detail(name)
            .emit();
    }
}
