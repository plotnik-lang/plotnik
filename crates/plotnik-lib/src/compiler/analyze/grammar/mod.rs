#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Grammar linking: resolve node-kind references against a tree-sitter grammar.

mod admissibility;
mod diagnostics;
pub mod grammar_binding;
pub mod link;
mod resolve;
mod utils;

pub use grammar_binding::{GrammarBinding, GrammarBindingBuilder};
