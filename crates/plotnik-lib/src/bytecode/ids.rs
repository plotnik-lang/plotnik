//! Bytecode index newtypes.

use std::num::NonZeroU16;

/// Index into the String Table.
///
/// Uses NonZeroU16 to make StringId(0) unrepresentable - index 0 is
/// reserved for the easter egg and never referenced by instructions.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(transparent)]
pub struct StringId(pub NonZeroU16);

impl StringId {
    /// Create a new StringId. Panics if n == 0.
    #[inline]
    pub fn new(n: u16) -> Self {
        Self(NonZeroU16::new(n).expect("StringId cannot be 0"))
    }

    /// Get the raw u16 value.
    #[inline]
    pub fn get(self) -> u16 {
        self.0.get()
    }
}

/// Index into the Type Definition table.
/// All types (including builtins) are stored sequentially in TypeDefs.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(transparent)]
pub struct QTypeId(pub u16);
