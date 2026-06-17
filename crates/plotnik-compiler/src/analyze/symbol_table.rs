//! Symbol table: name resolution and reference checking.
//!
//! Two-pass approach:
//! 1. Collect all `Name = expr` definitions from all sources
//! 2. Check that all `(UpperIdent)` references are defined

use indexmap::IndexMap;

use crate::Diagnostics;
use crate::diagnostics::DiagnosticKind;
use crate::parser::{Root, ast, token_src};

use super::Reporter;
use super::visitor::Visitor;
use crate::query::{SourceId, SourceMap};

/// Sentinel name for unnamed definitions (bare expressions at root level).
/// Code generators can emit whatever name they want for this.
pub const UNNAMED_DEF: &str = "_";

#[derive(Clone, Debug, Default)]
pub struct SymbolTable {
    table: IndexMap<String, ast::Expr>,
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

    pub fn remove(&mut self, name: &str) -> Option<(SourceId, ast::Expr)> {
        let expr = self.table.shift_remove(name)?;
        let source_id = self.files.shift_remove(name)?;
        Some((source_id, expr))
    }

    pub fn contains(&self, name: &str) -> bool {
        self.table.contains_key(name)
    }

    pub fn get(&self, name: &str) -> Option<&ast::Expr> {
        self.table.get(name)
    }

    pub fn source_id(&self, name: &str) -> Option<SourceId> {
        self.files.get(name).copied()
    }

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

    /// Return the table's own borrow of the stored key for `name` — for callers
    /// that need a `&str` tied to the table's lifetime, not to their search string.
    pub fn lookup_key(&self, name: &str) -> Option<&str> {
        self.table.get_key_value(name).map(|(k, _)| k.as_str())
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

    for (&source_id, ast) in ast_map {
        let src = source_map.content(source_id);
        let mut resolver = ReferenceResolver {
            src,
            reporter: Reporter::new(source_id, diag),
            symbol_table: &mut symbol_table,
        };
        resolver.visit(ast);
    }

    for (&source_id, ast) in ast_map {
        let mut validator = ReferenceValidator {
            reporter: Reporter::new(source_id, diag),
            symbol_table: &symbol_table,
        };
        validator.visit(ast);
    }

    symbol_table
}

struct ReferenceResolver<'q, 'd, 'a> {
    src: &'q str,
    reporter: Reporter<'d>,
    symbol_table: &'a mut SymbolTable,
}

impl Visitor for ReferenceResolver<'_, '_, '_> {
    fn visit_def(&mut self, def: &ast::Def) {
        let Some(body) = def.body() else { return };

        if let Some(token) = def.name() {
            let name = token_src(&token, self.src);
            if self.symbol_table.contains(name) {
                self.reporter
                    .report(DiagnosticKind::DuplicateDefinition, token.text_range())
                    .message(name)
                    .emit();
            } else {
                let source_id = self.reporter.source();
                self.symbol_table.insert(name, source_id, body);
            }
        } else {
            // Parser already validates multiple unnamed defs; we keep the last one.
            if self.symbol_table.contains(UNNAMED_DEF) {
                self.symbol_table.remove(UNNAMED_DEF);
            }
            let source_id = self.reporter.source();
            self.symbol_table.insert(UNNAMED_DEF, source_id, body);
        }
    }
}

struct ReferenceValidator<'d, 'a> {
    reporter: Reporter<'d>,
    symbol_table: &'a SymbolTable,
}

impl Visitor for ReferenceValidator<'_, '_> {
    fn visit_ref(&mut self, r: &ast::Ref) {
        let Some(name_token) = r.name() else { return };
        let name = name_token.text();

        if self.symbol_table.contains(name) {
            return;
        }

        self.reporter
            .report(DiagnosticKind::UndefinedReference, name_token.text_range())
            .message(name)
            .emit();
    }
}
