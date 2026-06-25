//! Runtime errors and control-flow signals for VM execution.

use crate::bytecode::ModuleError;

/// Errors during VM execution.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("exceeded the step limit of {0} steps")]
    StepLimitExceeded(u64),

    /// `used` is the live-heap measurement at the trip point; because the arenas
    /// grow geometrically it can overshoot `limit` by up to a doubling, so it is
    /// reported alongside the ceiling to make the limit tunable.
    #[error("exceeded the memory limit of {limit} bytes (used {used} bytes)")]
    MemoryLimitExceeded { used: u64, limit: u64 },

    #[error("no match found")]
    NoMatch,

    #[error("module error: {0}")]
    Module(#[from] ModuleError),
}

/// Non-error outcomes that unwind a step back to the main execution loop.
///
/// These are propagated through the same `Err` channel as `RuntimeError` (so
/// `?` can short-circuit a step), but they are not failures: the main loop
/// either continues (`Backtracked`) or completes the run (`Accept`).
#[derive(Debug)]
pub(crate) enum ControlFlow {
    /// Successful completion: the run is done and the effect log is final.
    Accept,
    /// Backtracking occurred; control returns to the main loop to continue.
    Backtracked,
}

/// The `Err` channel of a VM step: either a control-flow signal or a real error.
#[derive(Debug)]
pub(crate) enum Signal {
    Flow(ControlFlow),
    Error(RuntimeError),
}

impl From<RuntimeError> for Signal {
    fn from(error: RuntimeError) -> Self {
        Signal::Error(error)
    }
}

impl From<ControlFlow> for Signal {
    fn from(flow: ControlFlow) -> Self {
        Signal::Flow(flow)
    }
}
