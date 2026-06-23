#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Reference analysis: dependency graph, recursion validation, ref collection.

pub mod dependencies;
mod recursion;
pub mod refs;

pub use recursion::validate_recursion;
