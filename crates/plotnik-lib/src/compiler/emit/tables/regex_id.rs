//! Regex table wire ID.

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub(in crate::compiler) struct RegexId(u16);

impl From<u16> for RegexId {
    #[inline]
    fn from(n: u16) -> Self {
        Self(n)
    }
}

impl TryFrom<usize> for RegexId {
    type Error = std::num::TryFromIntError;

    #[inline]
    fn try_from(n: usize) -> Result<Self, Self::Error> {
        u16::try_from(n).map(Self)
    }
}

impl From<RegexId> for u16 {
    #[inline]
    fn from(v: RegexId) -> Self {
        v.0
    }
}

impl std::fmt::Display for RegexId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
