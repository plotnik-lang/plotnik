//! Entry-point admission checks.

use crate::compiler::analyze::refs::DefinitionGraph;
use crate::core::Interner;
use indexmap::IndexMap;

use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::Root;

/// Report every source definition whose body never became an admitted pattern.
///
/// Positional assertions (`.`, `-field`, `.!`) are not patterns by themselves,
/// so definitions containing only one are absent from the definition graph. The
/// AST remains the source of truth for the original definition list.
pub fn check_entry_points(
    ast_map: &IndexMap<SourceId, Root>,
    interner: &Interner,
    definitions: &DefinitionGraph,
    diag: &mut Diagnostics,
) {
    let mut any_defs = false;
    for (source_id, root) in ast_map {
        for def in root.defs() {
            any_defs = true;
            let Some(name) = def.name() else { continue };
            if definitions.id_for_name(interner, name.text()).is_none() {
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
