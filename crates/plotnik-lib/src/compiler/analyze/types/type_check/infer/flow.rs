use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use rowan::TextRange;

use crate::compiler::diagnostics::diagnostics::DiagnosticKind;
use crate::compiler::diagnostics::source::SourceId;

use super::InferVisitor;
use super::super::def_id::Symbol;
use super::super::types::{FieldInfo, OutputFlow, TYPE_VOID, TypeId};

impl InferVisitor<'_, '_> {
    /// Fold `source` fields into `target` in place, reporting a diagnostic on any
    /// name collision. Shared by sequences and named nodes so both paths reject
    /// duplicate captures identically.
    pub(super) fn merge_fields(
        &mut self,
        source_id: SourceId,
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
                    self.ctx
                        .diag
                        .report(source_id, DiagnosticKind::DuplicateCaptureInScope, range)
                        .detail(self.ctx.interner.resolve(name))
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
        source: SourceId,
        merged_fields: BTreeMap<Symbol, FieldInfo>,
        output_children: Vec<(TextRange, TypeId)>,
        parent_range: TextRange,
    ) -> OutputFlow {
        let has_bubbles = !merged_fields.is_empty();

        match (has_bubbles, output_children.len()) {
            (false, 0) => OutputFlow::Void,
            (false, 1) => OutputFlow::Value(output_children[0].1),
            (false, _) => {
                self.report_ambiguous_outputs(source, parent_range, &output_children);
                OutputFlow::Void
            }
            (true, 0) => OutputFlow::Fields(self.ctx.type_ctx.intern_struct(merged_fields)),
            (true, _) => {
                self.report_uncaptured_output_with_captures(source, &output_children);
                OutputFlow::Fields(self.ctx.type_ctx.intern_struct(merged_fields))
            }
        }
    }
}
