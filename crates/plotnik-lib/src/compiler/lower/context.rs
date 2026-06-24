use crate::compiler::analyze::grammar::GrammarBinding;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::core::Interner;

/// Shared state for lower construction, cleanup passes, and verification.
pub(in crate::compiler::lower) struct CompileCtx<'a> {
    pub(in crate::compiler::lower) interner: &'a Interner,
    pub(in crate::compiler::lower) type_ctx: &'a TypeAnalysis,
    pub(in crate::compiler::lower) symbol_table: &'a SymbolTable,
    pub(in crate::compiler::lower) grammar: &'a GrammarBinding,
    pub(in crate::compiler::lower) dependency_analysis: &'a DependencyAnalysis,
}
