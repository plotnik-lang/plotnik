//! `TypeAnalysis` and its builder live in `analyze::types::type_analysis`;
//! re-exported here as the `context` module the type-check pass internals build
//! against.

pub use crate::compiler::analyze::types::type_analysis::{TypeAnalysis, TypeAnalysisBuilder};
