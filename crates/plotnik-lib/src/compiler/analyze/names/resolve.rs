//! Name-resolution pass: collect definitions, then check references.
//!
//! Two passes over every source:
//! 1. Collect all `Name = pattern` definitions.
//! 2. Check that every `(UpperIdent)` reference resolves to a definition.

use crate::compiler::analyze::Located;
use crate::compiler::analyze::shape::validation::ValidatedAst;
use crate::compiler::analyze::visitor::Visitor;
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::parse::ast::{self, token_src};
use crate::core::utils::find_similar;

use super::CollectedDefinitions;

pub(in crate::compiler) fn resolve_names(
    validated: &ValidatedAst<'_>,
    diag: &mut Diagnostics,
) -> CollectedDefinitions {
    let mut definitions = CollectedDefinitions::default();

    for (&source_id, ast) in validated.ast_map() {
        let src = validated.source_map().content(source_id);
        let mut resolver = DefCollector {
            src,
            diag: &mut *diag,
            definitions: &mut definitions,
        };
        resolver.visit(&Located::new(source_id, ast.clone()));
    }

    for (&source_id, ast) in validated.ast_map() {
        let mut validator = ReferenceValidator {
            diag: &mut *diag,
            definitions: &definitions,
        };
        validator.visit(&Located::new(source_id, ast.clone()));
    }

    definitions
}

struct DefCollector<'q, 'd, 'a> {
    src: &'q str,
    diag: &'d mut Diagnostics,
    definitions: &'a mut CollectedDefinitions,
}

impl Visitor for DefCollector<'_, '_, '_> {
    fn visit_def(&mut self, def: &Located<ast::Def>) {
        let Some(body) = def.node().body() else {
            return;
        };
        // A nameless def is a parser-level error (MissingDefName); there is no name
        // to resolve, so it never enters the table.
        let Some(token) = def.node().name() else {
            return;
        };

        let name = token_src(&token, self.src);
        if let Some(first) = self.definitions.definition_span(name) {
            self.diag
                .report(
                    DiagnosticKind::DuplicateDefinition,
                    def.span_of(def.node().syntax().text_range()),
                )
                .related_to(first, "first defined here")
                .detail(name)
                .emit();
        } else {
            self.definitions.insert(name, def.source(), body);
        }
    }
}

struct ReferenceValidator<'d, 'a> {
    diag: &'d mut Diagnostics,
    definitions: &'a CollectedDefinitions,
}

impl Visitor for ReferenceValidator<'_, '_> {
    fn visit_def_ref(&mut self, r: &Located<ast::DefRef>) {
        let Some(name_token) = r.node().name() else {
            return;
        };
        let name = name_token.text();

        if self.definitions.defined_name(name).is_some() {
            return;
        }

        let candidates: Vec<&str> = self.definitions.names_in_declaration_order().collect();
        let mut builder = self.diag.report(
            DiagnosticKind::UndefinedReference,
            r.span_of(name_token.text_range()),
        );
        if let Some(similar) = find_similar(name, &candidates) {
            builder = builder.fix(
                format!("replace with the defined reference `({similar})`"),
                similar,
            );
        } else {
            builder = builder.hint(format!(
                "define `{name}` before using `({name})`, or change the reference to an existing definition"
            ));
        }
        builder.detail(name).emit();
    }
}
