//! Reference analysis: dependency graph, recursion validation, ref collection.

pub mod collect;
pub mod dependencies;
pub mod dependency_analysis;
mod recursion;

pub use dependency_analysis::DependencyAnalysis;
pub use recursion::validate_recursion;

#[cfg(test)]
mod collect_tests;
