#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Type inference: structural arity and data-flow types computed over the AST.

pub mod capture_kind;
mod entrypoints;
mod naming;
pub mod type_analysis;
pub mod type_check;
pub mod type_shape;

pub use capture_kind::CaptureKind;
pub use entrypoints::check_entrypoints;
pub use type_analysis::TypeAnalysis;
pub use type_shape::TypeShape;
