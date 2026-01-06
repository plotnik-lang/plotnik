//! TypeScript type emitter from bytecode Module.
//!
//! Converts compiled bytecode back to TypeScript declarations.
//! Used as a test oracle and for generating types from .ptkq files.

mod analysis;
mod config;
mod convert;
mod emitter;
mod naming;
mod render;

pub use config::{Config, VoidType};
pub use emitter::Emitter;

use crate::bytecode::Module;

/// Emit TypeScript from a bytecode module.
pub fn emit(module: &Module) -> String {
    Emitter::new(module, Config::default()).emit()
}

/// Emit TypeScript from a bytecode module with custom config.
pub fn emit_with_config(module: &Module, config: Config) -> String {
    Emitter::new(module, config).emit()
}
