#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Epsilon-transition elimination over the compiled IR.

mod eliminate;

pub use eliminate::eliminate_epsilons;
