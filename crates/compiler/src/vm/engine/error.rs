use plotnik_runtime::CallFrameError;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
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
