use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use rowan::TextRange;

use crate::compiler::analyze::types::type_shape::FieldInfo;
use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::core::Symbol;

use super::InferVisitor;

impl InferVisitor<'_, '_> {
    /// Add one field to a scope, reporting a diagnostic if the name is already
    /// bound. `range` locates the offending capture for the caret. This is the
    /// single duplicate-capture gate: sequences call it per child, a named node
    /// calls it for its own capture bubbling alongside the children.
    pub(super) fn insert_scope_field(
        &mut self,
        target: &mut BTreeMap<Symbol, FieldInfo>,
        name: Symbol,
        info: FieldInfo,
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
        target: &mut BTreeMap<Symbol, FieldInfo>,
        source: &BTreeMap<Symbol, FieldInfo>,
        range: TextRange,
    ) {
        for (&name, &info) in source {
            self.insert_scope_field(target, name, info, range);
        }
    }
}
