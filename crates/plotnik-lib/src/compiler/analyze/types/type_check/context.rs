//! `TypeAnalysis` and its builder live in `compiler::core`; re-exported here
//! as the `context` module the type-check pass internals build against.

pub use crate::compiler::core::{TypeAnalysis, TypeAnalysisBuilder};
