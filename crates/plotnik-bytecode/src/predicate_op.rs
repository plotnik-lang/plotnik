//! Predicate operators for bytecode.
//!
//! Extracted from parser for use by both compiler and runtime.
//! The runtime needs to decode predicate operators from bytecode.

/// Predicate operator for node text filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PredicateOp {
    /// `==` - equals
    Eq,
    /// `!=` - not equals
    Ne,
    /// `^=` - starts with
    StartsWith,
    /// `$=` - ends with
    EndsWith,
    /// `*=` - contains
    Contains,
    /// `=~` - regex match
    RegexMatch,
    /// `!~` - regex no match
    RegexNoMatch,
}

impl PredicateOp {
    /// Decode from bytecode representation.
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => Self::Eq,
            1 => Self::Ne,
            2 => Self::StartsWith,
            3 => Self::EndsWith,
            4 => Self::Contains,
            5 => Self::RegexMatch,
            6 => Self::RegexNoMatch,
            _ => panic!("invalid predicate op byte: {b}"),
        }
    }

    /// Encode for bytecode.
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Eq => 0,
            Self::Ne => 1,
            Self::StartsWith => 2,
            Self::EndsWith => 3,
            Self::Contains => 4,
            Self::RegexMatch => 5,
            Self::RegexNoMatch => 6,
        }
    }

    /// Operator as display string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::StartsWith => "^=",
            Self::EndsWith => "$=",
            Self::Contains => "*=",
            Self::RegexMatch => "=~",
            Self::RegexNoMatch => "!~",
        }
    }

    /// Check if this is a regex operator.
    pub fn is_regex_op(&self) -> bool {
        matches!(self, Self::RegexMatch | Self::RegexNoMatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_roundtrip() {
        for op in [
            PredicateOp::Eq,
            PredicateOp::Ne,
            PredicateOp::StartsWith,
            PredicateOp::EndsWith,
            PredicateOp::Contains,
            PredicateOp::RegexMatch,
            PredicateOp::RegexNoMatch,
        ] {
            assert_eq!(PredicateOp::from_byte(op.to_byte()), op);
        }
    }

    #[test]
    fn as_str() {
        assert_eq!(PredicateOp::Eq.as_str(), "==");
        assert_eq!(PredicateOp::Ne.as_str(), "!=");
        assert_eq!(PredicateOp::StartsWith.as_str(), "^=");
        assert_eq!(PredicateOp::EndsWith.as_str(), "$=");
        assert_eq!(PredicateOp::Contains.as_str(), "*=");
        assert_eq!(PredicateOp::RegexMatch.as_str(), "=~");
        assert_eq!(PredicateOp::RegexNoMatch.as_str(), "!~");
    }

    #[test]
    fn is_regex_op() {
        assert!(!PredicateOp::Eq.is_regex_op());
        assert!(!PredicateOp::Ne.is_regex_op());
        assert!(!PredicateOp::StartsWith.is_regex_op());
        assert!(!PredicateOp::EndsWith.is_regex_op());
        assert!(!PredicateOp::Contains.is_regex_op());
        assert!(PredicateOp::RegexMatch.is_regex_op());
        assert!(PredicateOp::RegexNoMatch.is_regex_op());
    }
}
