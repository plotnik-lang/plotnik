use crate::compiler::analyze::grammar::GrammarBinding;
use crate::compiler::analyze::refs::DefinitionGraph;
use crate::compiler::analyze::shape::PatternFacts;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::DefinitionOutput;
use crate::compiler::ids::DefId;
use crate::core::Interner;

/// Shared artifacts produced by semantic analysis and consumed by later phases.
#[derive(Clone, Copy)]
pub(crate) struct AnalysisArtifacts<'a> {
    pub(crate) interner: &'a Interner,
    pub(crate) type_analysis: &'a TypeAnalysis,
    pub(crate) pattern_facts: &'a PatternFacts,
    pub(crate) definitions: &'a DefinitionGraph,
    pub(crate) grammar: &'a GrammarBinding,
}

impl<'a> AnalysisArtifacts<'a> {
    /// Entry-point-eligible outputs in the existing `DefId` order.
    pub(crate) fn iter_entry_point_outputs(
        self,
    ) -> impl Iterator<Item = (DefId, DefinitionOutput)> + 'a {
        let type_analysis = self.type_analysis;
        let pattern_facts = self.pattern_facts;

        type_analysis
            .iter_def_output()
            .filter(move |&(def_id, _)| pattern_facts.is_entry_point_eligible(def_id))
    }
}
