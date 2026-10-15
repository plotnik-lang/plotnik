use rowan::TextRange;

use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::span::Span;

#[derive(Clone, Debug)]
pub struct Located<T> {
    source: SourceId,
    node: T,
}

impl<T> Located<T> {
    pub fn new(source: SourceId, node: T) -> Self {
        Self { source, node }
    }

    pub fn source(&self) -> SourceId {
        self.source
    }

    pub fn node(&self) -> &T {
        &self.node
    }

    pub fn wrap<U>(&self, child: U) -> Located<U> {
        Located {
            source: self.source,
            node: child,
        }
    }

    pub fn span_of(&self, range: TextRange) -> Span {
        Span::new(self.source, range)
    }
}
