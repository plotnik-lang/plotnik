//! TypeScript type emitter from bytecode Module.
//!
//! Converts compiled bytecode back to TypeScript declarations.
//! Used as a test oracle and for generating types from .ptkq files.

mod analysis;
mod config;
mod convert;
mod emitter;
mod render;

pub use config::{Config, VoidType};
pub use emitter::{DtsRange, Emitter};

use crate::bytecode::Module;

pub fn emit(module: &Module, config: Config) -> String {
    Emitter::new(module, config).emit()
}

pub fn emit_mapped(module: &Module, config: Config) -> (String, Vec<DtsRange>) {
    assert!(
        config.colors.blue.is_empty()
            && config.colors.green.is_empty()
            && config.colors.dim.is_empty()
            && config.colors.reset.is_empty(),
        "mapped TypeScript emission requires colors off"
    );
    Emitter::new(module, config).emit_mapped()
}
