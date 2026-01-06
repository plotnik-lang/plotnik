//! Bytecode emission from compiled queries.
//!
//! Converts the compiled IR into the binary bytecode format. This module handles:
//! - String table construction and interning
//! - Type table building with field resolution
//! - Cache-aligned instruction layout
//! - Section assembly and header generation
//!
//! Entry points:
//! - [`emit`]: Emit bytecode without language linking
//! - [`emit_linked`]: Emit bytecode with node type/field validation

mod emitter;
mod error;
pub mod layout;
mod string_table;
mod type_table;

#[cfg(all(test, feature = "plotnik-langs"))]
mod emit_tests;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod string_table_tests;
#[cfg(test)]
mod type_table_tests;

pub use emitter::{emit, emit_linked};
pub use error::EmitError;
pub use string_table::StringTableBuilder;
pub use type_table::TypeTableBuilder;
