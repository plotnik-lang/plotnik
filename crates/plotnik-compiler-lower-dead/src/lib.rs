#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Dead-code elimination: remove unreachable steps from the compiled IR.

mod dce;

pub use dce::remove_unreachable;
