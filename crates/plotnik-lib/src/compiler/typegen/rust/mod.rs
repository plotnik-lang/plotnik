//! Rust type generation for the proc-macro backend.
//!
//! The TypeScript peer renders the bytecode module's type table; this one
//! deliberately consumes the analysis-level model instead, where
//! `TypeShape::Ref` recursion cut points still exist — `Box` placement falls
//! out of the ref graph with no cycle reconstruction, and nominal names come
//! verbatim from the naming pass. Type inference needs no grammar, so the
//! output is exactly the query's typing, independent of the target language.

mod analysis;
mod config;
mod emitter;
mod model;
mod serde_impls;

#[cfg(test)]
mod analysis_tests;

pub use config::Config;
pub(crate) use model::{TypeContext, TypeModel};

use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::core::Interner;

pub(crate) fn emit(
    types: &TypeAnalysis,
    deps: &DependencyAnalysis,
    interner: &Interner,
    config: &Config,
) -> String {
    let schema = crate::compiler::analyze::output::OutputSchema::new(types, deps, interner)
        .expect("bytecode dry-run validated the output schema");
    let model = TypeModel::new(schema);
    emit_model(&model, config)
}

pub(crate) fn emit_model(model: &TypeModel<'_>, config: &Config) -> String {
    emitter::Emitter::new(model, config).emit()
}
