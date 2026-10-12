//! Up-navigation collapse: merge consecutive `Up` moves in the compiled IR.

mod up;

pub use up::collapse_up;
