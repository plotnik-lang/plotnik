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
mod def_id_tests;
#[cfg(test)]
mod unify_tests;

pub use capture_mechanism::CaptureMechanism;
pub use context::{TypeAnalysis, TypeAnalysisBuilder};
pub use def_id::{DefId, Interner, Symbol};
pub use types::{
    Arity, FieldInfo, OutputFlow, PatternResult, QuantifierKind, TYPE_NODE, TYPE_VOID, TypeId,
    TypeShape,
};
pub use unify::{UnifyError, unify_flow, unify_flows};

use plotnik_compiler_core::DependencyAnalysis;
use plotnik_compiler_core::SymbolTable;
use plotnik_compiler_diagnostics::diagnostics::Diagnostics;

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
