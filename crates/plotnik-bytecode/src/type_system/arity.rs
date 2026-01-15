//! Structural arity definitions.
//!
//! Arity tracks whether an expression matches one or many node positions.

/// Structural arity - whether an expression matches one or many positions.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Arity {
    /// Exactly one node position.
    One,
    /// Multiple sequential positions.
    Many,
}

impl Arity {
    /// Combine arities: Many wins.
    ///
    /// When combining expressions, if either has Many arity,
    /// the result has Many arity.
    pub fn combine(self, other: Self) -> Self {
        if self == Self::One && other == Self::One {
            return Self::One;
        }
        Self::Many
    }

    /// Check if this is singular arity.
    pub fn is_one(self) -> bool {
        self == Self::One
    }

    /// Check if this is plural arity.
    pub fn is_many(self) -> bool {
        self == Self::Many
    }
}
