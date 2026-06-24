#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Grammar linking: resolve node-kind references against a tree-sitter grammar.

mod check;
mod diagnostics;
pub mod binding;
pub mod link;
mod resolve;
mod utils;

pub use binding::{GrammarBinding, GrammarBindingBuilder};

#[cfg(test)]
mod link_tests;
