//! Symbol table: name resolution and reference checking.
//!
//! Two-pass approach:
//! 1. Collect all `Name = expr` definitions from all sources
//! 2. Check that all `(UpperIdent)` references are defined

use indexmap::IndexMap;

use crate::Diagnostics;
use crate::diagnostics::DiagnosticKind;
use crate::parser::{Root, ast, token_src};

use super::visitor::Visitor;
use crate::query::source_map::{SourceId, SourceMap};

/// Sentinel name for unnamed definitions (bare expressions at root level).
/// Code generators can emit whatever name they want for this.
pub const UNNAMED_DEF: &str = "_";

/// Registry of named definitions in a query.
///
/// Stores the mapping from definition names to their AST expressions,
/// along with source file information for diagnostics.
#[derive(Clone, Debug, Default)]
pub struct SymbolTable {
    /// Maps symbol name to its AST expression.
    table: IndexMap<String, ast::Expr>,
    /// Maps symbol name to the source file where it's defined.
    files: IndexMap<String, SourceId>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a symbol definition.
    ///
    /// Returns `true` if the symbol was newly inserted, `false` if it already existed
    /// (in which case the old value is replaced).
    pub fn insert(&mut self, name: &str, source_id: SourceId, expr: ast::Expr) -> bool {
        let is_new = !self.table.contains_key(name);
        self.table.insert(name.to_owned(), expr);
        self.files.insert(name.to_owned(), source_id);
        is_new
    }

    /// Remove a symbol definition.
    pub fn remove(&mut self, name: &str) -> Option<(SourceId, ast::Expr)> {
        let expr = self.table.shift_remove(name)?;
        let source_id = self.files.shift_remove(name)?;
        Some((source_id, expr))
    }

    /// Check if a symbol is defined.
    pub fn contains(&self, name: &str) -> bool {
        self.table.contains_key(name)
    }

    /// Get the expression for a symbol.
    pub fn get(&self, name: &str) -> Option<&ast::Expr> {
        self.table.get(name)
    }

    /// Get the source file where a symbol is defined.
    pub fn source_id(&self, name: &str) -> Option<SourceId> {
        self.files.get(name).copied()
    }

    /// Get both the source ID and expression for a symbol.
    pub fn get_full(&self, name: &str) -> Option<(SourceId, &ast::Expr)> {
        let expr = self.table.get(name)?;
        let source_id = self.files.get(name).copied()?;
        Some((source_id, expr))
    }

    /// Number of symbols in the symbol table.
    pub fn len(&self) -> usize {
        self.table.len()
    }

    /// Check if the symbol table is empty.
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    /// Iterate over symbol names in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.table.keys().map(String::as_str)
    }

    /// Iterate over (name, expr) pairs in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ast::Expr)> {
        self.table.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Iterate over (name, source_id, expr) tuples in insertion order.
    pub fn iter_full(&self) -> impl Iterator<Item = (&str, SourceId, &ast::Expr)> {
        self.table.iter().map(|(k, v)| {
            let source_id = self.files[k];
            (k.as_str(), source_id, v)
        })
    }
}

pub fn resolve_names(
    source_map: &SourceMap,
    ast_map: &IndexMap<SourceId, Root>,
    diag: &mut Diagnostics,
) -> SymbolTable {
    let mut symbol_table = SymbolTable::new();

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
        let mut validator = ReferenceValidator {
            source_id,
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
    symbol_table: &'t mut SymbolTable,
}

impl Visitor for ReferenceResolver<'_, '_, '_> {
    fn visit_def(&mut self, def: &ast::Def) {
        let Some(body) = def.body() else { return };

        if let Some(token) = def.name() {
            // Named definition: `Name = ...`
            let name = token_src(&token, self.src);
            if self.symbol_table.contains(name) {
                self.diag
                    .report(
                        self.source_id,
                        DiagnosticKind::DuplicateDefinition,
                        token.text_range(),
                    )
                    .message(name)
                    .emit();
            } else {
                self.symbol_table.insert(name, self.source_id, body);
            }
        } else {
            // Unnamed definition: `...` (root expression)
            // Parser already validates multiple unnamed defs; we keep the last one.
            if self.symbol_table.contains(UNNAMED_DEF) {
                self.symbol_table.remove(UNNAMED_DEF);
            }
            self.symbol_table.insert(UNNAMED_DEF, self.source_id, body);
        }
    }
}

struct ReferenceValidator<'d, 't> {
    source_id: SourceId,
    diag: &'d mut Diagnostics,
    symbol_table: &'t SymbolTable,
}

impl Visitor for ReferenceValidator<'_, '_> {
    fn visit_ref(&mut self, r: &ast::Ref) {
        let Some(name_token) = r.name() else { return };
        let name = name_token.text();

        if self.symbol_table.contains(name) {
            return;
        }

        self.diag
            .report(
                self.source_id,
                DiagnosticKind::UndefinedReference,
                name_token.text_range(),
            )
            .message(name)
            .emit();
    }
}
