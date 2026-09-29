//! Grammar binding: resolve node-kind references against a tree-sitter grammar.

pub mod bind;
pub mod binding;
mod diagnostics;
mod model;
mod resolve;
mod satisfiability;
mod utils;
mod validate;

pub use binding::{GrammarBinding, GrammarBindingBuilder};
pub use satisfiability::DEFAULT_SATISFIABILITY_WORK_BUDGET;
