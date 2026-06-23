//! Entrypoint admissibility checks derived from inferred definition types.

use std::collections::HashSet;

use indexmap::IndexMap;
use plotnik_compiler_core::DependencyAnalysis;
use plotnik_core::{Interner, Symbol};

use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::parser::Root;
use crate::source::SourceId;

use super::type_check::TypeAnalysis;

/// Report every definition that compiles to no entrypoint.
///
/// Value-less bodies (`.`, `-field`, `.!`) produce no type, so they are absent
/// from `TypeAnalysis::iter_def_types()`. The AST is the source of truth for the
/// original definition list, including definitions that never reached the symbol
/// table.
pub fn validate_entrypoints(
    ast_map: &IndexMap<SourceId, Root>,
    interner: &Interner,
    type_analysis: &TypeAnalysis,
    dependency_analysis: &DependencyAnalysis,
    diag: &mut Diagnostics,
) {
    let typed: HashSet<Symbol> = type_analysis
        .iter_def_types()
        .map(|(def_id, _)| dependency_analysis.def_name_sym(def_id))
        .collect();

    for (source_id, root) in ast_map {
        for def in root.defs() {
            let Some(name) = def.name() else { continue };
            let has_entrypoint = interner
                .get(name.text())
                .is_some_and(|sym| typed.contains(&sym));
            if !has_entrypoint {
                diag.report(*source_id, DiagnosticKind::NoEntrypoints, name.text_range())
                    .emit();
            }
        }
    }
}
