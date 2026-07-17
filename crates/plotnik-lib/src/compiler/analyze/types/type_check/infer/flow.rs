use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use crate::compiler::analyze::types::inference_flow::InferredField;
use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::Pattern;
use crate::core::Symbol;

use super::InferVisitor;

#[derive(Default)]
pub(super) struct ScopeFields {
    entries: BTreeMap<Symbol, InferredField>,
}

impl ScopeFields {
    fn insert(&mut self, name: Symbol, field: InferredField) -> Result<(), Span> {
        match self.entries.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(field);
                Ok(())
            }
            Entry::Occupied(entry) => Err(entry.get().first_name_span()),
        }
    }

    pub(super) fn into_fields(self) -> BTreeMap<Symbol, InferredField> {
        self.entries
    }
}

impl InferVisitor<'_, '_> {
    fn insert_scope_field(&mut self, target: &mut ScopeFields, name: Symbol, field: InferredField) {
        let name_span = field.first_name_span();
        let Err(first_name_span) = target.insert(name, field) else {
            return;
        };

        let field = self.ctx.interner.resolve(name).to_string();
        self.report(DiagnosticKind::DuplicateCaptureInScope, name_span.range)
            .detail(field)
            .related_to(first_name_span, "first captured here")
            .emit();
    }

    pub(super) fn merge_scope_fields(
        &mut self,
        target: &mut ScopeFields,
        source_pattern: Pattern,
        source: &crate::compiler::analyze::types::inference_flow::InferredFieldFlow,
    ) {
        for (&name, field) in &source.fields {
            self.insert_scope_field(
                target,
                name,
                InferredField::forwarded(field.info, source_pattern.clone(), name, field),
            );
        }
    }
}
