//! Grammar metadata derived from tree-sitter `grammar.json` files.

mod admissibility;
mod aliases;
mod identity;
mod json;
mod lower;
mod node_shapes;
mod prepared;
mod productions;
pub mod raw;
mod render;
mod rules;
mod structure;
mod symbols;
mod tokens;
mod types;
mod validation;

pub use identity::GrammarIdentity;
pub use json::GrammarError;
pub use prepared::VariableType;
pub use render::{
    Body, DEFAULT_WIDTH, Def, DefKind, DumpOptions, GrammarNodeRef, Quant, Shape, TokenText,
    TreeGrammar,
};
pub(crate) use structure::{NodeRealizer, StepProjection};
pub use structure::{SkeletonStep, SkeletonVariable, StepTarget, StructureTable, VarId};
pub use types::Grammar;
