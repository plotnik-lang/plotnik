//! Node classification for sibling-skipping decisions.

/// Runtime/analyzer view of a tree node for sibling-skipping decisions.
///
/// At runtime these bits come from one tree-sitter node instance. In grammar
/// analysis they are an approximation by node kind; that boundary is explicit at
/// the call site that constructs the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeClass {
    pub anonymous: bool,
    pub extra: bool,
}

/// What kind of sibling may be skipped while searching for the next match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipClass {
    Any,
    Trivia,
    Extras,
    Exact,
}

impl SkipClass {
    pub fn admits(self, node: NodeClass) -> bool {
        match self {
            Self::Any => true,
            Self::Trivia => node.anonymous || node.extra,
            Self::Extras => node.extra,
            Self::Exact => false,
        }
    }
}
