use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use rowan::TextRange;

use crate::compiler::analyze::types::type_shape::RecordField;
use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::cst::SyntaxNode;
use crate::core::Symbol;

use super::InferVisitor;
use super::diagnostics::capture_site_map;

struct ScopeField {
    value: RecordField,
    first_site: TextRange,
}

impl ScopeField {
    fn new(value: RecordField, first_site: TextRange) -> Self {
        Self { value, first_site }
    }
}

#[derive(Default)]
pub(super) struct ScopeFields {
    entries: BTreeMap<Symbol, ScopeField>,
}

impl ScopeFields {
    fn insert(
        &mut self,
        name: Symbol,
        value: RecordField,
        site: TextRange,
    ) -> Result<(), TextRange> {
        match self.entries.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(ScopeField::new(value, site));
                Ok(())
            }
            Entry::Occupied(entry) => Err(entry.get().first_site),
        }
    }

    pub(super) fn into_fields(self) -> BTreeMap<Symbol, RecordField> {
        self.entries
            .into_iter()
            .map(|(name, field)| (name, field.value))
            .collect()
    }
}

impl InferVisitor<'_, '_> {
    fn insert_scope_field(
        &mut self,
        target: &mut ScopeFields,
        name: Symbol,
        value: RecordField,
        site: TextRange,
    ) {
        let Err(first_site) = target.insert(name, value, site) else {
            return;
        };

        let field = self.ctx.interner.resolve(name).to_string();
        let source = self.source;
        self.report(DiagnosticKind::DuplicateCaptureInScope, site)
            .detail(field)
            .related_to(Span::new(source, first_site), "first captured here")
            .emit();
    }

    /// Fold `source` fields into `target` in one pass over their capture
    /// provenance, rejecting name collisions.
    pub(super) fn merge_scope_fields(
        &mut self,
        target: &mut ScopeFields,
        source: &BTreeMap<Symbol, RecordField>,
        source_root: &SyntaxNode,
    ) {
        let sites = capture_site_map(source_root, self.ctx.interner);
        for (&name, &value) in source {
            let site = *sites
                .get(&name)
                .expect("bubbling result field has a capture site in its source scope");
            self.insert_scope_field(target, name, value, site);
        }
    }
}
