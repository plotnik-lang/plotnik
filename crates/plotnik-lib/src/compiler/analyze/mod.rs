mod artifacts;
pub mod anchors;
pub mod grammar;
pub mod located;
pub mod names;
pub mod refs;
pub mod shape;
pub mod types;
pub mod visitor;

pub(crate) use artifacts::AnalysisArtifacts;
pub use located::Located;

#[cfg(test)]
mod anchors_tests;
