#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Dead-code elimination: remove unreachable steps from the compiled IR.

mod dead_code;

pub use dead_code::remove_unreachable;
