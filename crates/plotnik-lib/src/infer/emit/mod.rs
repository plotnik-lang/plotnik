//! Code emitters for inferred types.
//!
//! This module provides language-specific code generation from a `TypeTable`.

pub mod rust;
pub mod typescript;

#[cfg(test)]
mod rust_tests;
#[cfg(test)]
mod typescript_tests;

pub use rust::{Indirection, RustEmitConfig, emit_rust};
pub use typescript::{OptionalStyle, TypeScriptEmitConfig, emit_typescript};
