//! Quantifier kinds for type inference.
//!
//! Quantifiers determine cardinality: how many times a pattern can match.

/// Quantifier kind for pattern matching.
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
    /// Whether this quantifier requires strict dimensionality (row capture).
    ///
    /// `*` and `+` produce arrays, so internal captures need explicit row structure.
    /// `?` produces at most one value, so no dimensionality issue.
    pub fn requires_row_capture(self) -> bool {
        matches!(self, Self::ZeroOrMore | Self::OneOrMore)
    }

    /// Whether this quantifier guarantees at least one match.
    pub fn is_non_empty(self) -> bool {
        matches!(self, Self::OneOrMore)
    }

    /// Whether this quantifier can match zero times.
    pub fn can_be_empty(self) -> bool {
        matches!(self, Self::Optional | Self::ZeroOrMore)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_row_capture() {
        assert!(!QuantifierKind::Optional.requires_row_capture());
        assert!(QuantifierKind::ZeroOrMore.requires_row_capture());
        assert!(QuantifierKind::OneOrMore.requires_row_capture());
    }

    #[test]
    fn is_non_empty() {
        assert!(!QuantifierKind::Optional.is_non_empty());
        assert!(!QuantifierKind::ZeroOrMore.is_non_empty());
        assert!(QuantifierKind::OneOrMore.is_non_empty());
    }

    #[test]
    fn can_be_empty() {
        assert!(QuantifierKind::Optional.can_be_empty());
        assert!(QuantifierKind::ZeroOrMore.can_be_empty());
        assert!(!QuantifierKind::OneOrMore.can_be_empty());
    }
}
