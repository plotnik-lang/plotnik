//! Type inference for query output types.
//!
//! This module provides:
//! - `TypeTable`: collection of inferred types
//! - `TypeKey` / `TypeValue`: type representation
//! - `TypePrinter`: builder for emitting types as code

pub mod emit;
mod printer;
mod types;
pub mod tyton;

#[cfg(test)]
mod types_tests;
#[cfg(test)]
mod tyton_tests;

pub use emit::{Indirection, OptionalStyle, RustEmitConfig, TypeScriptEmitConfig};
pub use printer::{RustPrinter, TypePrinter, TypeScriptPrinter};
pub use types::{MergedField, TypeKey, TypeTable, TypeValue};
