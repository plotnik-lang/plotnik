use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use rowan::TextRange;

use crate::compiler::diagnostics::diagnostics::DiagnosticKind;

use super::super::def_id::Symbol;
use super::super::types::{FieldInfo, OutputFlow, TYPE_VOID, TypeId};
use super::InferVisitor;

impl InferVisitor<'_, '_> {
    /// Fold `source` fields into `target` in place, reporting a diagnostic on any
    /// name collision. Shared by sequences and named nodes so both paths reject
    /// duplicate captures identically.
    pub(super) fn merge_fields(
        &mut self,
        target: &mut BTreeMap<Symbol, FieldInfo>,
        source: &BTreeMap<Symbol, FieldInfo>,
        range: TextRange,
    ) {
        for (&name, &info) in source {
            match target.entry(name) {
                Entry::Vacant(e) => {
                    e.insert(info);
                }
                Entry::Occupied(_) => {
                    let field = self.ctx.interner.resolve(name).to_string();
                    self.report(DiagnosticKind::DuplicateCaptureInScope, range)
                        .detail(field)
                        .emit();
                }
            }
        }
    }

    pub(super) fn flow_to_type(&mut self, flow: &OutputFlow) -> TypeId {
        match flow {
            OutputFlow::Void => TYPE_VOID,
            OutputFlow::Value(t) | OutputFlow::Fields(t) => *t,
        }
    }

    /// Compute flow from merged bubble fields and output-producing children.
    ///
    /// Rules:
    /// - No bubbles, 0 outputs -> Void
    /// - No bubbles, 1 output -> Forward output (propagate)
    /// - No bubbles, 2+ outputs -> Error (ambiguous)
    /// - Bubbles, 0 outputs -> Fields(struct)
    /// - Bubbles, 1+ outputs -> Error (require capture)
    pub(super) fn compute_merged_flow(
        &mut self,
        merged_fields: BTreeMap<Symbol, FieldInfo>,
        output_children: Vec<(TextRange, TypeId)>,
        parent_range: TextRange,
    ) -> OutputFlow {
        let has_bubble_fields = !merged_fields.is_empty();
        if !has_bubble_fields {
            return match output_children.as_slice() {
                [] => OutputFlow::Void,
                [(_, type_id)] => OutputFlow::Value(*type_id),
                _ => {
                    self.report_ambiguous_outputs(parent_range, &output_children);
                    OutputFlow::Void
                }
            };
        }

        let merged_type = self.ctx.type_ctx.intern_struct(merged_fields);
        if output_children.is_empty() {
            return OutputFlow::Fields(merged_type);
        }

        self.report_uncaptured_output_with_captures(&output_children);
        OutputFlow::Fields(merged_type)
    }
}
