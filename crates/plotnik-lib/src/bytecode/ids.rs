//! Bytecode index newtypes.

use super::constants::{STEP_ACCEPT, STEP_SIZE};

/// Index into the Transitions section (8-byte steps).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(transparent)]
pub struct StepId(pub u16);

impl StepId {
    pub const ACCEPT: Self = Self(STEP_ACCEPT);

    #[inline]
    pub fn is_accept(self) -> bool {
        self.0 == STEP_ACCEPT
    }

    #[inline]
    pub fn byte_offset(self) -> usize {
        self.0 as usize * STEP_SIZE
    }
}

/// Index into the String Table.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(transparent)]
pub struct StringId(pub u16);

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
        assert_eq!(StepId(0).byte_offset(), 0);
        assert_eq!(StepId(1).byte_offset(), 8);
        assert_eq!(StepId(10).byte_offset(), 80);
    }
}
