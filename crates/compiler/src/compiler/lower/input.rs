use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::result::ResultModel;

pub(crate) struct LowerInput<'a> {
    pub(crate) analysis: AnalysisArtifacts<'a>,
    pub(crate) result: &'a ResultModel,
    pub(crate) inspection: bool,
}
