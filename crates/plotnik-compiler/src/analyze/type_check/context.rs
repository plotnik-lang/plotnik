//! `TypeAnalysis` and its `TypeAnalysisBuilder` live in `plotnik-compiler-core`;
//! re-exported here so the type-check pass and its consumers keep referring to
//! them as `crate::analyze::type_check::{TypeAnalysis, TypeAnalysisBuilder}`.

pub use plotnik_compiler_core::{TypeAnalysis, TypeAnalysisBuilder};
