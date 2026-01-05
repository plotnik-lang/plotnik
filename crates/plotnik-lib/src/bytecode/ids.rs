//! Bytecode index newtypes.

use std::num::NonZeroU16;

use super::constants::STEP_SIZE;

/// Index into the Transitions section (8-byte steps).
///
/// Step 0 is a valid address (preamble starts there).
/// In successor fields, raw value 0 means "terminal" — this sentinel
/// is handled by decoding logic, not by the type.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(transparent)]
pub struct StepId(pub u16);

impl StepId {
    #[inline]
    pub fn new(n: u16) -> Self {
        Self(n)
    }

    /// Get the raw u16 value.
    #[inline]
    pub fn get(self) -> u16 {
        self.0
    }

    #[inline]
    pub fn byte_offset(self) -> usize {
        self.0 as usize * STEP_SIZE
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_id_byte_offset() {
        assert_eq!(StepId::new(0).byte_offset(), 0);
        assert_eq!(StepId::new(1).byte_offset(), 8);
        assert_eq!(StepId::new(10).byte_offset(), 80);
    }
}
