//! Unified type checking pass.
//!
//! Computes both structural arity (for field validation) and data flow types
//! (for TypeScript emission) in a single traversal.

mod context;
mod infer;
mod symbol;
pub(crate) mod types;
mod unify;

#[cfg(test)]
mod context_tests;
#[cfg(test)]
mod symbol_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod unify_tests;

pub use context::TypeContext;
pub use symbol::{DefId, Interner, Symbol};
pub use types::{
    Arity, FieldInfo, QuantifierKind, TYPE_NODE, TYPE_STRING, TYPE_VOID, TermInfo, TypeFlow,
    TypeId, TypeShape,
};
pub use unify::{UnifyError, unify_flow, unify_flows};

use indexmap::IndexMap;

use crate::analyze::dependencies::DependencyAnalysis;
use crate::analyze::symbol_table::{SymbolTable, UNNAMED_DEF};
use crate::diagnostics::Diagnostics;
use crate::parser::Root;
use crate::query::source_map::SourceId;

/// Run type inference on all definitions.
///
/// Processes definitions in dependency order (leaves first) to handle
/// recursive definitions correctly.
pub fn infer_types(
    interner: &mut Interner,
    ast_map: &IndexMap<SourceId, Root>,
    symbol_table: &SymbolTable,
    dependency_analysis: &DependencyAnalysis,
    diag: &mut Diagnostics,
) -> TypeContext {
    infer::InferencePass::new(interner, ast_map, symbol_table, dependency_analysis, diag).run()
}

/// Get the primary definition name (first non-underscore, or underscore if none).
pub fn primary_def_name(symbol_table: &SymbolTable) -> &str {
    for name in symbol_table.keys() {
        if name != UNNAMED_DEF {
            return name;
        }
    }

    UNNAMED_DEF
}
