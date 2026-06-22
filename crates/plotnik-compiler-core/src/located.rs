//! A node paired with the source it lives in.
//!
//! A `TextRange` is meaningless without the `SourceId` it indexes into. `Located`
//! binds the two so a location carries its source by construction: passing a node
//! across a workspace-file boundary cannot silently misattribute its diagnostics.

use rowan::TextRange;

use crate::source::SourceId;
use crate::span::Span;

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

    /// Re-tag a value reached from this node with the SAME source — a child of a
    /// node always lives in the same file as the node. Crossing into another
    /// file is done with a fresh `Located::new`, never `wrap`.
    pub fn wrap<U>(&self, child: U) -> Located<U> {
        Located {
            source: self.source,
            node: child,
        }
    }

    /// Pair a range from this node's source with that source.
    pub fn span_of(&self, range: TextRange) -> Span {
        Span::new(self.source, range)
    }
}
