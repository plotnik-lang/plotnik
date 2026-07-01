use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use rowan::TextRange;

use crate::compiler::analyze::types::type_shape::FieldInfo;
use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::core::Symbol;

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
}
