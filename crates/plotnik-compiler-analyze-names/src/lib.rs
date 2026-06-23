#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Name resolution: build the symbol table from the validated AST.

pub mod symbol_table;

pub use symbol_table::{SymbolTable, resolve_names};
