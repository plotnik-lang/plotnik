mod config;
mod ident;
mod literal;
mod model;
mod module;
mod publish;
mod representation;
mod template;
mod type_model;
mod types;
mod types_config;

pub(crate) use config::Config;
pub(crate) use module::generate;
pub(crate) use type_model::TypeModel;
pub(crate) use types_config::Config as TypesConfig;

use crate::compiler::analyze::result::ResultSchema;

pub(crate) fn emit_types(schema: ResultSchema<'_>, config: &TypesConfig) -> String {
    let types = TypeModel::new(schema);
    let decode = crate::compiler::emit::decode::ResultDecodePlan::build(&schema);
    let model = model::ResultModel::new(&schema, &types, &decode);
    emit_model(&model, config)
}

fn emit_model(model: &model::ResultModel, config: &TypesConfig) -> String {
    types::Emitter::new(model, config).emit()
}
