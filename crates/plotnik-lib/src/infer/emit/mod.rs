//! Code emitters for inferred types.
//!
//! This module provides language-specific code generation from a `TypeTable`.

pub mod rust;
#[cfg(test)]
pub mod rust_tests;
pub mod typescript;
#[cfg(test)]
pub mod typescript_tests;

pub use rust::{Indirection, RustEmitConfig, emit_rust};
pub use typescript::{OptionalStyle, TypeScriptEmitConfig, emit_typescript};
