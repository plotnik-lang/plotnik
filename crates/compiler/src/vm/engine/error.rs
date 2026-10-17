use plotnik_runtime::LimitExceeded;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Limit(#[from] LimitExceeded),

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

impl From<LimitExceeded> for Signal {
    fn from(error: LimitExceeded) -> Self {
        Signal::Error(RuntimeError::Limit(error))
    }
}

impl From<ControlFlow> for Signal {
    fn from(flow: ControlFlow) -> Self {
        Signal::Flow(flow)
    }
}
