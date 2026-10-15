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
