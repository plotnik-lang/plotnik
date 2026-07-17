//! Unified type checking pass.
//!
//! Computes static root extent and result flow in one traversal.

mod infer;
mod root_extent;
mod unify;

pub use crate::compiler::analyze::types::RootExtent;
pub use crate::compiler::analyze::types::type_analysis::TypeAnalysis;
pub use crate::core::Interner;
pub(crate) use infer::definition_value_root;

use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::shape::anchor_context::AnchorContextAnalysis;
use crate::compiler::analyze::types::naming::{RawTypeNameValidator, TypeNamer};
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
    let anchor_contexts = AnchorContextAnalysis::new(interner, symbol_table, dependency_analysis);
    let structural_facts = infer::StructuralFacts::analyze(
        interner,
        symbol_table,
        dependency_analysis,
        &anchor_contexts,
    );

    let pass = infer::InferPassEnv {
        interner,
        symbol_table,
        dependency_analysis,
        structural_facts: &structural_facts,
        diag,
    };
    let mut types = infer::InferPass::new(pass).run();

    if types.has_built_in_capture_types() {
        RawTypeNameValidator::new(&mut types, interner).validate(symbol_table, dependency_analysis);
    }
    types.normalize_capture_types(interner, diag);
    TypeNamer::new(&mut types, interner, diag).assign(symbol_table, dependency_analysis);
    types.finish()
}
