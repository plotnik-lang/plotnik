#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Instruction deduplication: merge structurally identical states in the compiled IR.

mod states;

pub use states::dedup_states;
