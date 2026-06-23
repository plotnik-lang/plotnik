#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Up-navigation collapse: merge consecutive `Up` moves in the compiled IR.

mod collapse_up;

#[cfg(test)]
mod collapse_up_tests;

pub use collapse_up::collapse_up;
