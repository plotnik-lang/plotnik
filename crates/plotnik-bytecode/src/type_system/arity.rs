//! Structural arity definitions.

/// Structural arity - whether an expression matches one or many positions.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Arity {
    /// Exactly one node position.
    One,
    /// Multiple sequential positions.
    Many,
}

impl Arity {
    /// Many wins: either Many → result is Many.
    pub fn combine(self, other: Self) -> Self {
        if self == Self::One && other == Self::One {
            return Self::One;
        }
        Self::Many
    }

    pub fn is_one(self) -> bool {
        self == Self::One
    }

    pub fn is_many(self) -> bool {
        self == Self::Many
    }
}
