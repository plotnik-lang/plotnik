//! Target-neutral plans consumed by generated-code backends.
//!
//! Lowering decides what the query means. This layer turns that meaning into
//! plain, ordered data so source renderers only choose a target language's
//! representation and syntax.

mod matcher;
mod replay;

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::output::OutputSchema;
use crate::compiler::lower::ir::NfaGraph;

pub(crate) use matcher::{
    CallPlan, CheckPlan, EffectPlan, FlowPlan, KindClass, MatchPlan, MatcherPlan, PredicatePlan,
    PredicateValuePlan, RegexId, StateId, StateOrigin, StatePlan, StatePlanKind,
};
pub(crate) use replay::{
    ReplayItem, ReplayItemKind, ReplayPlan, ReplayScopePlan, ReplayValuePlan, ReplayVariantPlan,
};

/// Everything a generated module shares across target languages.
pub(crate) struct ModulePlan<'a> {
    artifacts: AnalysisArtifacts<'a>,
    output: OutputSchema<'a>,
    matcher: MatcherPlan,
    replay: ReplayPlan,
}

impl<'a> ModulePlan<'a> {
    pub(crate) fn build(graph: &NfaGraph, artifacts: AnalysisArtifacts<'a>) -> Self {
        let output = OutputSchema::from_artifacts(artifacts)
            .expect("bytecode dry-run validated the output schema");
        let matcher = MatcherPlan::build(graph, artifacts, output.layout());
        let replay = ReplayPlan::build(&output);
        Self {
            artifacts,
            output,
            matcher,
            replay,
        }
    }

    pub(crate) fn artifacts(&self) -> AnalysisArtifacts<'a> {
        self.artifacts
    }

    pub(crate) fn output(&self) -> &OutputSchema<'a> {
        &self.output
    }

    pub(crate) fn matcher(&self) -> &MatcherPlan {
        &self.matcher
    }

    pub(crate) fn replay(&self) -> &ReplayPlan {
        &self.replay
    }
}
