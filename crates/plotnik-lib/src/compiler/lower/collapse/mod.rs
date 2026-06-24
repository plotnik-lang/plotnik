#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Up-navigation collapse: merge consecutive `Up` moves in the compiled IR.

mod up;

#[cfg(test)]
mod up_tests;

pub use up::collapse_up;
