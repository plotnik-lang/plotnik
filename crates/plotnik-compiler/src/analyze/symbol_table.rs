//! Symbol table: name resolution and reference checking.
//!
//! Two-pass approach:
//! 1. Collect all `Name = pattern` definitions from all sources
//! 2. Check that all `(UpperIdent)` references are defined

use indexmap::IndexMap;

use crate::Diagnostics;
use crate::diagnostics::DiagnosticKind;
use crate::parser::{Root, ast, token_src};

use super::Located;
use super::visitor::Visitor;
use crate::query::{SourceId, SourceMap};

/// Sentinel name for unnamed definitions (bare expressions at root level).
/// Code generators can emit whatever name they want for this.
pub const UNNAMED_DEF: &str = "_";

#[derive(Clone, Debug, Default)]
pub struct SymbolTable {
    table: IndexMap<String, ast::Pattern>,
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
    pub fn insert(&mut self, name: &str, source_id: SourceId, pattern: ast::Pattern) -> bool {
        let is_new = !self.table.contains_key(name);
        self.table.insert(name.to_owned(), pattern);
        self.files.insert(name.to_owned(), source_id);
        is_new
    }

    pub fn remove(&mut self, name: &str) -> Option<(SourceId, ast::Pattern)> {
        let pattern = self.table.shift_remove(name)?;
        let source_id = self.files.shift_remove(name)?;
        Some((source_id, pattern))
    }

    pub fn contains(&self, name: &str) -> bool {
        self.table.contains_key(name)
    }

    pub fn body(&self, name: &str) -> Option<&ast::Pattern> {
        self.table.get(name)
    }

    pub fn source_id(&self, name: &str) -> Option<SourceId> {
        self.files.get(name).copied()
    }

    pub fn definition(&self, name: &str) -> Option<(SourceId, &ast::Pattern)> {
        let pattern = self.table.get(name)?;
        let source_id = self.files.get(name).copied()?;
        Some((source_id, pattern))
    }

    /// A definition's body bound to the source it lives in, so a pass crossing a
    /// reference into another workspace file carries the target's source with the
    /// node instead of tracking an ambient "current source".
    pub(crate) fn located_definition(&self, name: &str) -> Option<Located<ast::Pattern>> {
        let (source_id, pattern) = self.definition(name)?;
        Some(Located::new(source_id, pattern.clone()))
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

    /// Iterate over (name, pattern) pairs in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ast::Pattern)> {
        self.table.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Iterate over (name, source_id, pattern) tuples in insertion order.
    pub fn definitions(&self) -> impl Iterator<Item = (&str, SourceId, &ast::Pattern)> {
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
            diag: &mut *diag,
            symbol_table: &mut symbol_table,
        };
        resolver.visit(&Located::new(source_id, ast.clone()));
    }

    for (&source_id, ast) in ast_map {
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
    symbol_table: &'a mut SymbolTable,
}

impl Visitor for ReferenceResolver<'_, '_, '_> {
    fn visit_def(&mut self, def: &Located<ast::Def>) {
        let Some(body) = def.node().body() else { return };

        if let Some(token) = def.node().name() {
            let name = token_src(&token, self.src);
            if self.symbol_table.contains(name) {
                self.diag
                    .report(def.source(), DiagnosticKind::DuplicateDefinition, token.text_range())
                    .detail(name)
                    .emit();
            } else {
                self.symbol_table.insert(name, def.source(), body);
            }
        } else {
            // Parser already validates multiple unnamed defs; we keep the last one.
            if self.symbol_table.contains(UNNAMED_DEF) {
                self.symbol_table.remove(UNNAMED_DEF);
            }
            self.symbol_table.insert(UNNAMED_DEF, def.source(), body);
        }
    }
}

struct ReferenceValidator<'d, 'a> {
    diag: &'d mut Diagnostics,
    symbol_table: &'a SymbolTable,
}

impl Visitor for ReferenceValidator<'_, '_> {
    fn visit_ref(&mut self, r: &Located<ast::Ref>) {
        let Some(name_token) = r.node().name() else { return };
        let name = name_token.text();

        if self.symbol_table.contains(name) {
            return;
        }

        self.diag
            .report(r.source(), DiagnosticKind::UndefinedReference, name_token.text_range())
            .detail(name)
            .emit();
    }
}
