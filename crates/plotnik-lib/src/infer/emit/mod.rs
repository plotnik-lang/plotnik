//! Code emitters for inferred types.
//!
//! This module provides language-specific code generation from a `TypeTable`.

pub mod rust;
pub mod typescript;

pub use rust::{Indirection, RustEmitConfig, emit_rust};
pub use typescript::{OptionalStyle, TypeScriptEmitConfig, emit_typescript};
