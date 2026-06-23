#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Type inference: structural arity and data-flow types computed over the AST.

mod entrypoints;
pub mod type_check;

pub use entrypoints::validate_entrypoints;
pub use type_check::{TypeAnalysis, TypeAnalysisBuilder, infer_types};
