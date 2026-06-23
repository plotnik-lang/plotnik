#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Epsilon-transition elimination over the compiled IR.

mod epsilon_elim;

pub use epsilon_elim::eliminate_epsilons;
