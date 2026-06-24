//! Unified type checking pass.
//!
//! Computes both structural arity (for field validation) and data flow types
//! (for TypeScript emission) in a single traversal.

mod analysis;
mod capture_kind;
mod strings;
mod infer;
pub(crate) mod shapes;
mod unify;

#[cfg(test)]
mod analysis_tests;
#[cfg(test)]
mod unify_tests;

pub use analysis::TypeAnalysis;
pub use strings::Interner;
pub use shapes::Arity;

use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::diagnostics::report::Diagnostics;

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
    let analysis = infer::InferPassEnv {
        interner,
        symbol_table,
        dependency_analysis,
        diag,
    };
    infer::InferPass::new(analysis).run()
}
