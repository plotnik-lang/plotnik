//! Entrypoint admission checks.

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
pub fn check_entrypoints(
    ast_map: &IndexMap<SourceId, Root>,
    interner: &Interner,
    definitions: &DefinitionGraph,
    diag: &mut Diagnostics,
) {
    for (source_id, root) in ast_map {
        for def in root.defs() {
            let Some(name) = def.name() else {
                continue;
            };
            if definitions.id_for_name(interner, name.text()).is_none() {
                diag.report(
                    DiagnosticKind::NoEntrypoints,
                    Span::new(*source_id, name.text_range()),
                )
                .detail(format!(
                    "`{}` cannot be an entrypoint because its body does not match exactly one root node",
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
}
