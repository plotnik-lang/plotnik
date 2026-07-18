//! Transient definitions collected during name resolution.
//!
//! Names remain strings until dependency analysis assigns stable `DefId`s and
//! interns them in SCC order. The completed map is then consumed into the
//! definition graph; later compiler phases never retain this representation.

use indexmap::IndexMap;

use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast;

#[derive(Default)]
pub(in crate::compiler) struct CollectedDefinitions {
    entries: IndexMap<String, (SourceId, ast::Pattern)>,
}

impl CollectedDefinitions {
    pub(in crate::compiler::analyze) fn body(&self, name: &str) -> Option<&ast::Pattern> {
        self.entries.get(name).map(|(_, body)| body)
    }

    pub(in crate::compiler::analyze) fn definition_span(&self, name: &str) -> Option<Span> {
        self.entries
            .get(name)
            .map(|(source, body)| span_of_definition(*source, body))
    }

    pub(in crate::compiler::analyze) fn insert(
        &mut self,
        name: &str,
        source: SourceId,
        body: ast::Pattern,
    ) {
        let previous = self.entries.insert(name.to_owned(), (source, body));
        assert!(
            previous.is_none(),
            "collected definition insertion must not replace an existing name",
        );
    }

    pub(in crate::compiler::analyze) fn defined_name(&self, name: &str) -> Option<&str> {
        self.entries
            .get_key_value(name)
            .map(|(name, _)| name.as_str())
    }

    pub(in crate::compiler::analyze) fn names_in_declaration_order(
        &self,
    ) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub(in crate::compiler::analyze) fn into_entries_in_declaration_order(
        self,
    ) -> impl Iterator<Item = (String, SourceId, ast::Pattern)> {
        self.entries
            .into_iter()
            .map(|(name, (source, body))| (name, source, body))
    }
}

fn span_of_definition(source: SourceId, body: &ast::Pattern) -> Span {
    let definition = body
        .syntax()
        .parent()
        .and_then(ast::Def::cast)
        .expect("collected definition body belongs to a definition");
    Span::new(source, definition.syntax().text_range())
}
