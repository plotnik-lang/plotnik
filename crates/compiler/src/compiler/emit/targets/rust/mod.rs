mod config;
mod decode;
mod decoder_frame;
mod entry_names;
mod ident;
mod literal;
mod module;
mod representation;
mod serde_impls;
mod type_model;
mod types;
mod types_config;

pub(crate) use config::Config;
pub(crate) use ident::module_ident;
pub(crate) use module::{generate, generate_sources};
pub(crate) use type_model::{TypeContext, TypeModel};
pub(crate) use types_config::Config as TypesConfig;

use crate::compiler::analyze::result::ResultSchema;

pub(crate) fn emit_types(schema: ResultSchema<'_>, config: &TypesConfig) -> String {
    let model = TypeModel::new(schema);
    emit_model(&model, config)
}

pub(crate) fn emit_model(model: &TypeModel<'_>, config: &TypesConfig) -> String {
    types::Emitter::new(model, config).emit()
}
