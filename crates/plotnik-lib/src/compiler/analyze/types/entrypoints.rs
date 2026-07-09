//! Definition-output checks derived from inferred types.

use std::collections::HashSet;

use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::core::{Interner, Symbol};
use indexmap::IndexMap;

use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::Root;

use super::type_check::TypeAnalysis;

/// Report every definition that produces no inferred output.
///
/// Value-less bodies (`.`, `-field`, `.!`) produce no type, so they are absent
/// from `TypeAnalysis::iter_def_output()`. The AST is the source of truth for the
/// original definition list, including definitions that never reached the symbol
/// table.
pub fn check_entrypoints(
    ast_map: &IndexMap<SourceId, Root>,
    interner: &Interner,
    type_analysis: &TypeAnalysis,
    dependency_analysis: &DependencyAnalysis,
    diag: &mut Diagnostics,
) {
    let output_defs: HashSet<Symbol> = type_analysis
        .iter_def_output()
        .map(|(def_id, _)| dependency_analysis.def_name_sym(def_id))
        .collect();

    let mut any_defs = false;
    for (source_id, root) in ast_map {
        for def in root.defs() {
            any_defs = true;
            let Some(name) = def.name() else { continue };
            let has_output = interner
                .get(name.text())
                .is_some_and(|sym| output_defs.contains(&sym));
            if !has_output {
                diag.report(
                    DiagnosticKind::NoEntrypoints,
                    Span::new(*source_id, name.text_range()),
                )
                .emit();
            }
        }
    }

    // A defless query (empty file, comments only) has nothing for the loops
    // above to flag; without this it would validate silently.
    if !any_defs && let Some((source_id, root)) = ast_map.first() {
        diag.report(
            DiagnosticKind::EmptyQuery,
            Span::new(*source_id, root.syntax().text_range()),
        )
        .emit();
    }
}
