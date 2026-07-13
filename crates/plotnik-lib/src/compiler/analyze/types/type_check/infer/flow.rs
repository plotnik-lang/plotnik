use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use rowan::TextRange;

use crate::compiler::analyze::types::type_shape::RecordField;
use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::core::Symbol;

use super::InferVisitor;

impl InferVisitor<'_, '_> {
    /// Add one field to a scope, reporting a diagnostic if the name is already
    /// bound. `range` locates the offending capture for the caret. This is the
    /// duplicate-capture gate for merging independently inferred child flows.
    /// A node's own bubbling capture validates its destination earlier, before
    /// capture-type normalization, because that admission result gates whether
    /// the capture type may run at all.
    pub(super) fn insert_scope_field(
        &mut self,
        target: &mut BTreeMap<Symbol, RecordField>,
        name: Symbol,
        info: RecordField,
        range: TextRange,
    ) {
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

    /// Fold `source` fields into `target` in place, rejecting name collisions.
    pub(super) fn merge_scope_fields(
        &mut self,
        target: &mut BTreeMap<Symbol, RecordField>,
        source: &BTreeMap<Symbol, RecordField>,
        range: TextRange,
    ) {
        for (&name, &info) in source {
            self.insert_scope_field(target, name, info, range);
        }
    }
}
