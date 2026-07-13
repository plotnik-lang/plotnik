//! Grammar binding: resolve node-kind references against a tree-sitter grammar.

pub mod bind;
pub mod binding;
mod check;
mod diagnostics;
mod participation;
mod resolve;
mod satisfiability;
mod utils;

pub use binding::{GrammarBinding, GrammarBindingBuilder};
pub use satisfiability::DEFAULT_SATISFIABILITY_STEP_BUDGET;

#[cfg(test)]
mod bind_tests;
