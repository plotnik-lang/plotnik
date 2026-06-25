#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Instruction packing: lower the symbolic IR into its final packed form.

mod lower;

#[cfg(test)]
mod lower_tests;

pub use lower::pack_instructions;
