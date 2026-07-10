//! Rust source target.

mod config;
mod entry_names;
mod ident;
mod literal;
mod module;
mod reader_frame;
mod replay;
mod representation;
mod serde_impls;
mod template;
mod type_model;
mod types;
mod types_config;

#[cfg(test)]
mod ident_tests;
#[cfg(test)]
mod literal_tests;
#[cfg(test)]
mod module_tests;
#[cfg(test)]
mod representation_tests;
#[cfg(test)]
mod template_tests;

pub(crate) use config::Config;
pub use entry_names::entry_fn_name;
pub(crate) use module::generate;
pub(crate) use type_model::{TypeContext, TypeModel};
pub(crate) use types_config::Config as TypesConfig;

use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::core::Interner;

pub(crate) fn emit_types(
    types: &TypeAnalysis,
    deps: &DependencyAnalysis,
    interner: &Interner,
    config: &TypesConfig,
) -> String {
    let schema = crate::compiler::analyze::output::OutputSchema::new(types, deps, interner)
        .expect("target-neutral compilation validated the output schema");
    let model = TypeModel::new(schema);
    emit_model(&model, config)
}

pub(crate) fn emit_model(model: &TypeModel<'_>, config: &TypesConfig) -> String {
    types::Emitter::new(model, config).emit()
}
