//! Runtime errors and control-flow signals for VM execution.

use plotnik_bytecode::ModuleError;

/// Errors during VM execution.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
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
