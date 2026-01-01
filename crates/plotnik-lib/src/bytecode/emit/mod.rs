//! Code generation from bytecode Module.
//!
//! This module re-exports from [`crate::codegen`] for backwards compatibility.
//! New code should use [`crate::codegen::typescript`] directly.

// Re-export from codegen module for backwards compatibility
pub use crate::codegen::typescript::{
    Config as TsEmitConfig, Emitter as TsEmitter, emit as emit_typescript,
    emit_with_config as emit_typescript_with_config,
};
