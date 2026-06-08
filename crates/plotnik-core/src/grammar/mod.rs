//! Grammar types for tree-sitter grammars.
//!
//! This module provides types for representing tree-sitter `grammar.json` files,
//! with support for JSON deserialization and compact binary serialization.

mod json;
pub mod raw;
mod tree_sitter;
mod types;

#[cfg(test)]
mod json_tests;
#[cfg(test)]
mod node_shapes_tests;
#[cfg(test)]
mod types_tests;

pub use json::GrammarError;
pub use types::Grammar;
