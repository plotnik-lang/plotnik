//! Code generation from bytecode Module.
//!
//! This module provides emitters for generating code from compiled bytecode.
//! Currently supports TypeScript, with Rust planned.

mod typescript;

pub use typescript::{
    EmitConfig as TsEmitConfig, TsEmitter, emit_typescript, emit_typescript_with_config,
};
