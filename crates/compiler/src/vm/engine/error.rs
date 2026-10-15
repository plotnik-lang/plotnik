use plotnik_runtime::CallFrameError;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// The matcher exhausted its fuel before the run finished. The value is
    /// the resolved fuel limit for that run.
    #[error("exhausted the fuel limit of {0}")]
    OutOfFuel(u64),

    /// `used` is the sampled estimate for occupied runtime entries, not allocated
    /// heap capacity. Sampling can detect an overshoot after it occurs.
    #[error("exceeded the memory limit of {limit} bytes (used {used} bytes)")]
    MemoryLimitExceeded { used: u64, limit: u64 },

    #[error(transparent)]
    CallFrame(#[from] CallFrameError),

    #[error("no match found")]
    NoMatch,
}

#[derive(Debug)]
pub(crate) enum ControlFlow {
    Accept,
    Backtracked,
}

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
