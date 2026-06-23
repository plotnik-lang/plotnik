//! Quantifier kinds for type inference.

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum QuantifierKind {
    /// `?` or `??` - zero or one.
    Optional,
    /// `*` or `*?` - zero or more.
    ZeroOrMore,
    /// `+` or `+?` - one or more.
    OneOrMore,
}

impl QuantifierKind {
    /// `*` and `+` produce arrays; internal captures need explicit row structure.
    /// `?` produces at most one value, so no dimensionality constraint applies.
    pub fn requires_row_capture(self) -> bool {
        matches!(self, Self::ZeroOrMore | Self::OneOrMore)
    }

    pub fn is_non_empty(self) -> bool {
        matches!(self, Self::OneOrMore)
    }

    pub fn can_be_empty(self) -> bool {
        matches!(self, Self::Optional | Self::ZeroOrMore)
    }
}
