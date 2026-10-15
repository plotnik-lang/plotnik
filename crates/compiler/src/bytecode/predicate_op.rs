#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PredicateOp {
    Eq,
    Ne,
    StartsWith,
    EndsWith,
    Contains,
    RegexMatch,
    RegexNoMatch,
}

impl PredicateOp {
    pub fn try_from_byte(b: u8) -> Option<Self> {
        let op = match b {
            1 => Self::Eq,
            2 => Self::Ne,
            3 => Self::StartsWith,
            4 => Self::EndsWith,
            5 => Self::Contains,
            6 => Self::RegexMatch,
            7 => Self::RegexNoMatch,
            _ => return None,
        };
        Some(op)
    }

    pub fn to_byte(self) -> u8 {
        match self {
            Self::Eq => 1,
            Self::Ne => 2,
            Self::StartsWith => 3,
            Self::EndsWith => 4,
            Self::Contains => 5,
            Self::RegexMatch => 6,
            Self::RegexNoMatch => 7,
        }
    }

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

    pub fn is_regex_op(&self) -> bool {
        matches!(self, Self::RegexMatch | Self::RegexNoMatch)
    }
}
