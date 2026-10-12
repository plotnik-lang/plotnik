//! Instruction deduplication: merge structurally identical states in the compiled IR.

mod states;

pub use states::dedup_states;
