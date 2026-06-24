#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Grammar linking: resolve node-kind references against a tree-sitter grammar.

pub mod grammar_binding;
pub mod link;
mod utils;

pub use grammar_binding::{GrammarBinding, GrammarBindingBuilder};
