//! Static top-level extent of a query pattern.

/// Whether one successful match has exactly one top-level syntax-tree node.
///
/// `NotSingleNode` deliberately combines empty, multiple-node, and variable
/// extents: the compiler only needs to know whether a definition is eligible
/// as an entry point or must remain a fragment.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum RootExtent {
    SingleNode,
    NotSingleNode,
}

impl RootExtent {
    pub fn combine(self, other: Self) -> Self {
        if self == Self::SingleNode && other == Self::SingleNode {
            return Self::SingleNode;
        }
        Self::NotSingleNode
    }
}
