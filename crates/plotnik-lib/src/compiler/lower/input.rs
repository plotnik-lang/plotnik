use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::names::SymbolTable;

/// Inputs and shared read-only state for the lowering pipeline.
pub(crate) struct LowerInput<'a> {
    pub(crate) analysis: AnalysisArtifacts<'a>,
    pub(crate) symbol_table: &'a SymbolTable,
    #[allow(dead_code)]
    pub(crate) inspection: bool,
}
