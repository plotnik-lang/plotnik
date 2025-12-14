//! Errors that can occur during query execution.

#[derive(Debug, Clone, thiserror::Error)]
pub enum RuntimeError {
    /// Execution fuel exhausted (too many interpreter operations).
    #[error("runtime execution limit exceeded")]
    ExecFuelExhausted,

    /// Recursion fuel exhausted (too many nested definition calls).
    #[error("runtime recursion limit exceeded")]
    RecursionLimitExceeded,
}
