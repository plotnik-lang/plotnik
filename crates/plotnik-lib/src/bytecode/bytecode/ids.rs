//! Bytecode index newtypes.

use std::num::NonZeroU16;

use crate::core::ZeroIdError;

/// Index into the String Table.
///
/// Uses NonZeroU16 to make StringId(0) unrepresentable - index 0 is
/// reserved for the easter egg and never referenced by instructions.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct StringId(NonZeroU16);

impl From<NonZeroU16> for StringId {
    #[inline]
    fn from(n: NonZeroU16) -> Self { Self(n) }
}
impl From<StringId> for NonZeroU16 {
    #[inline]
    fn from(v: StringId) -> Self { v.0 }
}
impl From<StringId> for u16 {
    #[inline]
    fn from(v: StringId) -> Self { v.0.get() }
}
impl TryFrom<u16> for StringId {
    type Error = ZeroIdError;
    #[inline]
    fn try_from(n: u16) -> Result<Self, Self::Error> {
        NonZeroU16::new(n).map(Self).ok_or(ZeroIdError)
    }
}
impl std::fmt::Display for StringId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.get())
    }
}

/// Index into the Type Definition table.
/// All types (including builtins) are stored sequentially in TypeDefs.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(transparent)]
pub struct TypeId(u16);

impl From<u16> for TypeId {
    #[inline]
    fn from(n: u16) -> Self { Self(n) }
}
impl From<TypeId> for u16 {
    #[inline]
    fn from(v: TypeId) -> Self { v.0 }
}
impl std::fmt::Display for TypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
