#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[path = "../../plotnik-compiler/src/typegen/mod.rs"]
pub mod typegen;

pub use typegen::*;
