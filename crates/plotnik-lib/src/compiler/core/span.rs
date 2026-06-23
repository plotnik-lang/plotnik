use rowan::TextRange;

use crate::compiler::core::source::SourceId;

/// A location that knows which source it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub source: SourceId,
    pub range: TextRange,
}

impl Span {
    pub fn new(source: SourceId, range: TextRange) -> Self {
        Self { source, range }
    }
}
