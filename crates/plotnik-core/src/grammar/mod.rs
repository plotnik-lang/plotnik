//! Grammar metadata derived from tree-sitter `grammar.json` files.

mod aliases;
mod json;
mod lower;
mod nfa;
mod node_shapes;
mod prepared;
mod productions;
pub mod raw;
mod rules;
mod structure;
mod symbols;
mod tokens;
mod types;
mod validation;

#[cfg(test)]
mod json_tests;
#[cfg(test)]
mod node_shapes_tests;
#[cfg(test)]
mod structure_tests;

#[cfg(test)]
mod types_tests;

pub use json::GrammarError;
pub use structure::{StepTarget, StructStep, StructVariable, StructureTable, VarId};
pub use types::Grammar;
