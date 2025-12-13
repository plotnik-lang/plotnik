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

mod build;
mod construct;

#[cfg(test)]
mod build_tests;
#[cfg(test)]
mod construct_tests;

pub use build::{BuildEffect, BuildGraph, BuildMatcher, BuildNode, Fragment, NodeId, RefMarker};
pub use construct::{GraphConstructor, construct_graph};
