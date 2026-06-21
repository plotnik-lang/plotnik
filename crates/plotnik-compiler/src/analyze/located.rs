//! A node paired with the source it lives in.
//!
//! A `TextRange` is meaningless without the `SourceId` it indexes into. `Located`
//! binds the two so a location carries its source by construction: passing a node
//! across a workspace-file boundary cannot silently misattribute its diagnostics.

use rowan::TextRange;

use crate::diagnostics::Span;
use crate::source::SourceId;

#[derive(Clone, Debug)]
pub(crate) struct Located<T> {
    source: SourceId,
    node: T,
}

impl<T> Located<T> {
    pub(crate) fn new(source: SourceId, node: T) -> Self {
        Self { source, node }
    }

    pub(crate) fn source(&self) -> SourceId {
        self.source
    }

    pub(crate) fn node(&self) -> &T {
        &self.node
    }

    /// Re-tag a value reached from this node with the SAME source — a child of a
    /// node always lives in the same file as the node. Crossing into another
    /// file is done with a fresh `Located::new`, never `wrap`.
    pub(crate) fn wrap<U>(&self, child: U) -> Located<U> {
        Located {
            source: self.source,
            node: child,
        }
    }

    /// Pair a range from this node's source with that source.
    pub(crate) fn span_of(&self, range: TextRange) -> Span {
        Span::new(self.source, range)
    }
}
