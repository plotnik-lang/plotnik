//! Name-resolution registry produced by the symbol-table pass.

use indexmap::IndexMap;

use crate::compiler::core::ast;
use crate::compiler::core::located::Located;
use crate::compiler::core::source::SourceId;

/// Name-resolution registry: every named definition bound to its body AST and the
/// source file that defines it.
///
/// Immutable once analysis produces it; the name-resolution pass builds one
/// through its `SymbolTableBuilder`.
#[derive(Clone, Debug)]
pub struct SymbolTable {
    table: IndexMap<String, ast::Pattern>,
    files: IndexMap<String, SourceId>,
}

impl SymbolTable {
    /// Freeze finished name-resolution data into the registry. The pass-owned
    /// builder is the intended caller.
    pub(in crate::compiler) fn new(
        table: IndexMap<String, ast::Pattern>,
        files: IndexMap<String, SourceId>,
    ) -> Self {
        assert_eq!(
            table.len(),
            files.len(),
            "symbol-table body and source maps must have the same definitions",
        );
        assert!(
            table.keys().all(|name| files.contains_key(name)),
            "every symbol-table body must have a source file",
        );
        assert!(
            files.keys().all(|name| table.contains_key(name)),
            "every symbol-table source file must have a body",
        );

        Self { table, files }
    }

    /// Body of the definition named `name` — the question consumers ask most.
    pub fn body(&self, name: &str) -> Option<&ast::Pattern> {
        self.table.get(name)
    }

    /// Which file defines `name`.
    pub fn source_id(&self, name: &str) -> Option<SourceId> {
        self.files.get(name).copied()
    }

    /// A definition's body together with the file it lives in.
    pub fn definition(&self, name: &str) -> Option<(SourceId, &ast::Pattern)> {
        let pattern = self.table.get(name)?;
        let source_id = self.files.get(name).copied()?;
        Some((source_id, pattern))
    }

    /// A definition's body bound to the source it lives in, so a pass crossing a
    /// reference into another workspace file carries the target's source with the node.
    pub fn located_definition(&self, name: &str) -> Option<Located<ast::Pattern>> {
        let (source_id, pattern) = self.definition(name)?;
        Some(Located::new(source_id, pattern.clone()))
    }

    /// Whether `name` is defined, yielding the registry's own borrow of the
    /// canonical name — a `&str` tied to the table, not to the caller's lookup string.
    pub fn defined_name(&self, name: &str) -> Option<&str> {
        self.table.get_key_value(name).map(|(k, _)| k.as_str())
    }

    /// The defined names, in definition order — the vertex set for dependency analysis.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.table.keys().map(String::as_str)
    }

    /// Whether no definitions were resolved.
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    /// Number of resolved definitions.
    pub fn count(&self) -> usize {
        self.table.len()
    }
}
