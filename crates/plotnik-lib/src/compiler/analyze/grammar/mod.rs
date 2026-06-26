#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Grammar linking: resolve node-kind references against a tree-sitter grammar.

pub mod binding;
mod check;
mod diagnostics;
pub mod link;
mod resolve;
mod satisfy;
mod utils;

pub use binding::{GrammarBinding, GrammarBindingBuilder};

#[cfg(test)]
mod link_tests;
