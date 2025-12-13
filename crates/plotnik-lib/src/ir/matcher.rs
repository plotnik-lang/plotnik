//! Node matchers for transition graph.
//!
//! Matchers are purely for node matching - navigation is handled by `Nav`.

use super::{NodeFieldId, NodeTypeId, Slice};

/// Discriminant for matcher variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatcherKind {
    Epsilon,
    Node,
    Anonymous,
    Wildcard,
}

/// Matcher determines what node satisfies a transition.
///
/// Navigation (descend/ascend) is handled by `Nav`, not matchers.
#[repr(C, u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matcher {
    /// Matches without consuming input. Used for control flow transitions.
    Epsilon,

    /// Matches a named node by kind, optionally constrained by field.
    Node {
        kind: NodeTypeId,
        field: Option<NodeFieldId>,
        negated_fields: Slice<NodeFieldId>,
    },

    /// Matches an anonymous node by kind, optionally constrained by field.
    Anonymous {
        kind: NodeTypeId,
        field: Option<NodeFieldId>,
        negated_fields: Slice<NodeFieldId>,
    },

    /// Matches any node (named or anonymous).
    Wildcard,
}

impl Matcher {
    /// Returns true if this matcher consumes a node.
    #[inline]
    pub fn consumes_node(&self) -> bool {
        !matches!(self, Matcher::Epsilon)
    }

    /// Returns the discriminant kind.
    #[inline]
    pub fn kind(&self) -> MatcherKind {
        match self {
            Matcher::Epsilon => MatcherKind::Epsilon,
            Matcher::Node { .. } => MatcherKind::Node,
            Matcher::Anonymous { .. } => MatcherKind::Anonymous,
            Matcher::Wildcard => MatcherKind::Wildcard,
        }
    }

    /// Returns the node type ID for Node/Anonymous variants, `None` otherwise.
    #[inline]
    pub fn node_kind(&self) -> Option<NodeTypeId> {
        match self {
            Matcher::Node { kind, .. } | Matcher::Anonymous { kind, .. } => Some(*kind),
            _ => None,
        }
    }

    /// Returns the field constraint, if any.
    #[inline]
    pub fn field(&self) -> Option<NodeFieldId> {
        match self {
            Matcher::Node { field, .. } | Matcher::Anonymous { field, .. } => *field,
            _ => None,
        }
    }

    /// Returns the negated fields slice. Empty for Epsilon/Wildcard.
    #[inline]
    pub fn negated_fields(&self) -> Slice<NodeFieldId> {
        match self {
            Matcher::Node { negated_fields, .. } | Matcher::Anonymous { negated_fields, .. } => {
                *negated_fields
            }
            _ => Slice::empty(),
        }
    }
}
