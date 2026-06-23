#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Grammar linking: resolve node-kind references against a tree-sitter grammar.

pub mod link;
mod utils;

pub use link::{GrammarBinding, GrammarBindingBuilder, GrammarLinkCtx};
