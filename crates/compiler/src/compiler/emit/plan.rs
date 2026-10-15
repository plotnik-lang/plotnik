use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::result::ResultSchema;
use crate::compiler::lower::ir::NfaGraph;

pub(crate) use super::decode::{
    DecodeCase, DecodeItem, DecodeItemKind, DecodeScope, DecodeValue, ResultDecodePlan,
};
pub(crate) use super::matcher::{
    CallPlan, CheckPlan, EffectPlan, FlowPlan, KindClass, MatchPlan, MatcherPlan, PredicatePlan,
    PredicateValuePlan, RegexId, StateId, StateOrigin, StatePlan, StatePlanKind,
};

pub(crate) struct CodegenPlan<'a> {
    result: ResultSchema<'a>,
    matcher: MatcherPlan,
    decode: ResultDecodePlan,
}

impl<'a> CodegenPlan<'a> {
    pub(crate) fn build(
        graph: &NfaGraph,
        artifacts: AnalysisArtifacts<'a>,
        result: ResultSchema<'a>,
    ) -> Result<Self, String> {
        let matcher = MatcherPlan::build(graph, artifacts, result.layout())?;
        let decode = ResultDecodePlan::build(&result);
        Ok(Self {
            result,
            matcher,
            decode,
        })
    }

    pub(crate) fn result(&self) -> &ResultSchema<'a> {
        &self.result
    }

    pub(crate) fn matcher(&self) -> &MatcherPlan {
        &self.matcher
    }

    pub(crate) fn decode(&self) -> &ResultDecodePlan {
        &self.decode
    }
}
