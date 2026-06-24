use crate::compiler::AnalysisInput;
use crate::compiler::analyze::names::SymbolTable;

/// Inputs and shared read-only state for the lowering pipeline.
pub(crate) struct LowerInput<'a> {
    pub(crate) analysis: AnalysisInput<'a>,
    pub(crate) symbol_table: &'a SymbolTable,
}
