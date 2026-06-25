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

pub fn emit(module: &Module, config: Config) -> String {
    Emitter::new(module, config).emit()
}
