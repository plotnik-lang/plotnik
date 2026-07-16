//! Semantic validation for anchor placement.
//!
//! Definition boundaries are call boundaries, not automatic errors. A group or
//! definition may export a leading/trailing anchor for a caller to discharge
//! with a sibling or named-node boundary. Only exported anchors that remain
//! contextless across every definition composition are rejected here.

use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::shape::anchor_context::AnchorContextAnalysis;
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::span::Span;

pub(crate) struct AnchorValidationInput<'a, 'd> {
    pub analysis: &'a AnchorContextAnalysis<'a>,
    pub dependency_analysis: &'a DependencyAnalysis,
    pub diag: &'d mut Diagnostics,
}

pub(crate) fn validate_anchors(input: AnchorValidationInput<'_, '_>) -> bool {
    let context_free_roots = input
        .dependency_analysis
        .sccs()
        .iter()
        .flatten()
        .copied()
        .filter(|&def_id| !input.analysis.definition_requires_external_context(def_id));
    let discharged = input.dependency_analysis.reachable_from(context_free_roots);

    let mut valid = true;
    for &def_id in input.dependency_analysis.sccs().iter().flatten() {
        if !input.analysis.definition_requires_external_context(def_id)
            || discharged.contains(def_id)
        {
            continue;
        }

        let source = input.dependency_analysis.def_source_id(def_id);
        for range in input.analysis.exported_anchor_ranges(def_id) {
            valid = false;
            input
                .diag
                .report(
                    DiagnosticKind::AnchorWithoutContext,
                    Span::new(source, range),
                )
                .emit();
        }
    }
    valid
}
