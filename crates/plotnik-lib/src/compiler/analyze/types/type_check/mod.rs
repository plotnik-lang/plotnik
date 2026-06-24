//! Unified type checking pass.
//!
//! Computes both structural arity (for field validation) and data flow types
//! (for TypeScript emission) in a single traversal.

mod capture_mechanism;
mod context;
mod def_id;
mod infer;
pub(crate) mod types;
mod unify;

#[cfg(test)]
mod context_tests;
#[cfg(test)]
mod unify_tests;

pub use context::TypeAnalysis;
pub use def_id::Interner;
pub use types::Arity;

use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::diagnostics::diagnostics::Diagnostics;

/// Run type inference on all definitions.
///
/// Processes definitions in dependency order (leaves first) to handle
/// recursive definitions correctly.
pub fn infer_types(
    interner: &mut Interner,
    symbol_table: &SymbolTable,
    dependency_analysis: &DependencyAnalysis,
    diag: &mut Diagnostics,
) -> TypeAnalysis {
    let analysis = infer::InferPassInput {
        interner,
        symbol_table,
        dependency_analysis,
        diag,
    };
    infer::InferencePass::new(analysis).run()
}
