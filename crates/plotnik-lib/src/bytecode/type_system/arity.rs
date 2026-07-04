//! Structural arity definitions.

/// Structural arity — whether one match of an expression spans exactly one
/// node position.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Arity {
    /// Exactly one node position.
    One,
    /// Anything else: multiple sequential positions, or a variable range
    /// (quantified patterns match zero or more positions).
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
}
