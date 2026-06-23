//! Unified type checking pass.
//!
//! Computes both structural arity (for field validation) and data flow types
//! (for TypeScript emission) in a single traversal.

mod capture_mechanism;
mod context;
mod infer;
mod def_id;
pub(crate) mod types;
mod unify;

#[cfg(test)]
mod context_tests;
#[cfg(test)]
mod def_id_tests;
#[cfg(test)]
mod unify_tests;

pub use capture_mechanism::{CaptureMechanism, classify_capture_mechanism, ref_returns_structured};
pub use context::TypeContext;
pub use def_id::{DefId, Interner, Symbol};
pub use types::{
    Arity, FieldInfo, QuantifierKind, TYPE_NODE, TYPE_VOID, PatternResult, OutputFlow, TypeId, TypeShape,
};
pub use unify::{UnifyError, unify_flow, unify_flows};

use crate::analyze::dependencies::DependencyAnalysis;
use crate::analyze::symbol_table::SymbolTable;
use crate::diagnostics::Diagnostics;

/// Run type inference on all definitions.
///
/// Processes definitions in dependency order (leaves first) to handle
/// recursive definitions correctly.
pub fn infer_types(
    interner: &mut Interner,
    symbol_table: &SymbolTable,
    dependency_analysis: &DependencyAnalysis,
    diag: &mut Diagnostics,
) -> TypeContext {
    let analysis = infer::InferPassInput {
        interner,
        symbol_table,
        dependency_analysis,
        diag,
    };
    infer::InferencePass::new(analysis).run()
}
