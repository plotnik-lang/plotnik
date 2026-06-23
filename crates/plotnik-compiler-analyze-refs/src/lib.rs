#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Reference analysis: dependency graph, recursion validation, ref collection.

pub mod dependencies;
pub mod refs;
mod recursion;

pub use dependencies::{DependencyAnalysis, analyze_dependencies};
pub use recursion::validate_recursion;
