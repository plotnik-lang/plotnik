//! Build-time graph representation for query compilation.
//!
//! This module provides an intermediate graph representation between
//! the parsed AST and the final compiled IR. The graph is mutable during
//! construction and supports analysis passes like epsilon elimination.
//!
//! # Architecture
//!
//! ```text
//! AST (parser) → BuildGraph → [analysis passes] → CompiledQuery (ir)
//! ```
//!
//! The `BuildGraph` borrows strings from the source (`&'src str`).
//! String interning happens during emission to `CompiledQuery`.

mod analysis;
mod build;
mod construct;
mod optimize;
mod typing;

#[cfg(test)]
mod analysis_tests;
#[cfg(test)]
mod build_tests;
#[cfg(test)]
mod construct_tests;
#[cfg(test)]
mod optimize_tests;
#[cfg(test)]
mod typing_tests;

pub use analysis::{AnalysisResult, StringInterner, analyze};
pub use build::{BuildEffect, BuildGraph, BuildMatcher, BuildNode, Fragment, NodeId, RefMarker};
pub use construct::{GraphConstructor, construct_graph};
pub use optimize::{OptimizeStats, eliminate_epsilons};
pub use typing::{
    InferredMember, InferredTypeDef, TypeDescription, TypeInferenceResult, UnificationError,
    dump_types, infer_types,
};
