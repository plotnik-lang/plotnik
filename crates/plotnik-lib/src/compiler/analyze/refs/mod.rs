#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Reference analysis: dependency graph, recursion validation, ref collection.

pub mod collect;
pub mod dependencies;
mod recursion;

pub use recursion::validate_recursion;
