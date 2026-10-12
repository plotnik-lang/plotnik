//! Grammar binding: resolve node-kind references against a tree-sitter grammar.
//!
//! `bind` rejects unknown names and unsupported syntax before `model` resolves
//! matching structure. Captures remain source provenance, not matching steps.
//! `GrammarBinding` separately retains the ordered tables needed by emission.
//!
//! Satisfiability threads retained grammar productions through child automata.
//! Configurations keep path-specific sibling boundaries, and hidden-rule keys
//! also keep inherited fields. Public kinds and their realization bodies stay
//! separate so occurrence aliases do not borrow another node's structure.
//!
//! Acceptance means the structural model has not ruled out a match. Text values,
//! lexical competition, parser conflicts, and recovery are not fully modeled.
//! Sibling recursion is widened conservatively. Resource exhaustion is distinct
//! from rejection, and bounded diagnostics cannot change a completed verdict.

pub mod bind;
pub mod binding;
mod format;
mod model;
mod resolve;
mod satisfiability;
mod validate;

pub use binding::{GrammarBinding, GrammarBindingBuilder};
pub use satisfiability::DEFAULT_SATISFIABILITY_WORK_BUDGET;
