use crate::compiler::analyze::grammar::GrammarBinding;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::core::Interner;

/// Shared artifacts produced by semantic analysis and consumed by later phases.
#[derive(Clone, Copy)]
pub(crate) struct AnalysisArtifacts<'a> {
    pub(crate) interner: &'a Interner,
    pub(crate) type_analysis: &'a TypeAnalysis,
    pub(crate) dependency_analysis: &'a DependencyAnalysis,
    pub(crate) grammar: &'a GrammarBinding,
}
