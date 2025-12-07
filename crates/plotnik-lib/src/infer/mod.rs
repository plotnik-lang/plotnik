//! Type inference for query output types.
//!
//! This module provides:
//! - `TypeTable`: collection of inferred types
//! - `TypeKey` / `TypeValue`: type representation
//! - `emit_rust`: Rust code emitter
//! - `emit_typescript`: TypeScript code emitter

pub mod emit;
mod types;
pub mod tyton;

#[cfg(test)]
mod types_tests;
#[cfg(test)]
mod tyton_tests;

pub use emit::{
    Indirection, OptionalStyle, RustEmitConfig, TypeScriptEmitConfig, emit_rust, emit_typescript,
};
pub use types::{TypeKey, TypeTable, TypeValue};
