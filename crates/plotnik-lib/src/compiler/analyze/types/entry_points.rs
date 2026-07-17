//! Entry-point eligibility checks derived from inferred patterns.

use std::collections::HashSet;

use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::core::{Interner, Symbol};
use indexmap::IndexMap;

use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::Root;

use super::type_check::TypeAnalysis;

/// Report every definition whose body never reached pattern analysis.
///
/// Positional assertions (`.`, `-field`, `.!`) are not patterns by themselves,
/// so definitions containing only one are absent from
/// `TypeAnalysis::iter_def_output()`. The AST is the source of truth for the
/// original definition list, including definitions that never reached name
/// resolution.
pub fn check_entry_points(
    ast_map: &IndexMap<SourceId, Root>,
    interner: &Interner,
    type_analysis: &TypeAnalysis,
    dependency_analysis: &DependencyAnalysis,
    diag: &mut Diagnostics,
) {
    let analyzed_defs: HashSet<Symbol> = type_analysis
        .iter_def_output()
        .map(|(def_id, _)| dependency_analysis.def_name_sym(def_id))
        .collect();

    let mut any_defs = false;
    for (source_id, root) in ast_map {
        for def in root.defs() {
            any_defs = true;
            let Some(name) = def.name() else { continue };
            let was_analyzed = interner
                .get(name.text())
                .is_some_and(|sym| analyzed_defs.contains(&sym));
            if !was_analyzed {
                diag.report(
                    DiagnosticKind::NoEntryPoints,
                    Span::new(*source_id, name.text_range()),
                )
                .detail(format!(
                    "`{}` cannot be an entry point because its body does not match exactly one root node",
                    name.text()
                ))
                .hint(format!(
                    "make `{}` match exactly one node, with any anchors or field constraints inside that node pattern",
                    name.text()
                ))
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
