use crate::compiler::analyze::grammar::GrammarBinding;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::core::Interner;

/// Inputs and shared read-only state for the lowering pipeline.
pub(crate) struct LowerInput<'a> {
    pub(crate) interner: &'a Interner,
    pub(crate) type_ctx: &'a TypeAnalysis,
    pub(crate) symbol_table: &'a SymbolTable,
    pub(crate) grammar: &'a GrammarBinding,
    pub(crate) dependency_analysis: &'a DependencyAnalysis,
}
