//! Type inference for query output types.
//!
//! This module provides:
//! - `TypeTable`: collection of inferred types
//! - `TypeKey` / `TypeValue`: type representation
//! - `emit_rust`: Rust code emitter
//! - `emit_typescript`: TypeScript code emitter

pub mod emit_rs;
pub mod emit_ts;
mod types;

pub use emit_rs::{Indirection, RustEmitConfig, emit_rust};
pub use emit_ts::{OptionalStyle, TypeScriptEmitConfig, emit_typescript};
pub use types::{TypeKey, TypeTable, TypeValue};
