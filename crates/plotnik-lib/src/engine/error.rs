//! Runtime errors for VM execution.

use crate::bytecode::ModuleError;

/// Errors during VM execution.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// Internal signal for successful completion (not a real error).
    #[error("accept")]
    Accept,

    /// Internal signal that backtracking occurred (control returns to main loop).
    #[error("backtracked")]
    Backtracked,

    #[error("execution fuel exhausted after {0} steps")]
    ExecFuelExhausted(u32),

    #[error("recursion limit exceeded (depth {0})")]
    RecursionLimitExceeded(u32),

    #[error("no match found")]
    NoMatch,

    #[error("invalid entrypoint: {0}")]
    #[allow(dead_code)]
    InvalidEntrypoint(String),

    #[error("module error: {0}")]
    Module(#[from] ModuleError),
}
