//! Target-neutral emission planning and target implementations.

mod ansi;
mod matcher;
mod plan;
mod replay;
pub(crate) mod sink;
mod target;
pub(crate) mod targets;

#[cfg(test)]
mod sink_tests;
#[cfg(test)]
mod target_tests;

pub use target::{
    BytecodeConfig, BytecodeInspection, CodegenProvenance, CodegenTarget, Emission,
    EmitConfigError, EmitTarget, RustCodegenConfig, RustModuleOutput, RustTypesOutput,
    TypeScriptCodegenConfig, TypeScriptNodeRepresentation, TypeScriptTypesOutput,
};

pub(crate) use plan::CodegenPlan;
pub use targets::rust::entry_fn_name;
pub use targets::typescript::{DtsRange, VoidType as TypeScriptVoidType};
