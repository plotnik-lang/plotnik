//! Semantic validation for anchor placement.
//!
//! Definition boundaries are call boundaries, not automatic errors. A group or
//! definition may export a leading/trailing anchor for a caller to discharge
//! with a sibling or named-node boundary. Only exported anchors that remain
//! contextless across every definition composition are rejected here.

use crate::compiler::analyze::refs::DefinitionGraph;
use crate::compiler::analyze::shape::PatternFacts;
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::span::Span;
use crate::core::Interner;

pub(crate) struct AnchorValidationInput<'a, 'd> {
    pub pattern_facts: &'a PatternFacts,
    pub definitions: &'a DefinitionGraph,
    pub interner: &'a Interner,
    pub diag: &'d mut Diagnostics,
}

pub(crate) fn validate_anchors(input: AnchorValidationInput<'_, '_>) -> bool {
    let self_contained_roots = input.definitions.ids_in_def_id_order().filter(|&def_id| {
        !input
            .pattern_facts
            .definition_requires_external_anchor_context(def_id)
    });
    let discharged = input.definitions.reachable_from(self_contained_roots);

    let mut valid = true;
    for def_id in input.definitions.ids_in_def_id_order() {
        if !input
            .pattern_facts
            .definition_requires_external_anchor_context(def_id)
            || discharged.contains(def_id)
        {
            continue;
        }

        let source = input.definitions.definition(def_id).source();
        for range in
            input
                .pattern_facts
                .exported_anchor_ranges(def_id, input.definitions, input.interner)
        {
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
