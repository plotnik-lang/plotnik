//! Target-neutral emission planning and target implementations.

mod ansi;
mod decode;
mod matcher;
mod plan;
pub(crate) mod sink;
mod target;
pub(crate) mod targets;

pub use target::{
    BytecodeConfig, BytecodeInspection, CodegenProvenance, CodegenTarget, Emission,
    EmitConfigError, EmitTarget, RustCodegenConfig, RustModuleOutput, RustTypesOutput,
    TypeScriptCodegenConfig, TypeScriptNodeRepresentation, TypeScriptTypesOutput,
};

pub(crate) use plan::CodegenPlan;
pub use targets::rust::journal_fn_name;
pub use targets::typescript::{MatchOnlyType as TypeScriptMatchOnlyType, TypeScriptBinding};
