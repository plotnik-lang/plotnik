//! Bytecode index newtypes.

use super::constants::{STEP_ACCEPT, STEP_SIZE, TYPE_CUSTOM_START, TYPE_STRING};

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
/// Values 0-2 are builtins; 3+ index into TypeDefs.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(transparent)]
pub struct QTypeId(pub u16);

impl QTypeId {
    pub const VOID: Self = Self(super::constants::TYPE_VOID);
    pub const NODE: Self = Self(super::constants::TYPE_NODE);
    pub const STRING: Self = Self(TYPE_STRING);

    #[inline]
    pub fn is_builtin(self) -> bool {
        self.0 <= TYPE_STRING
    }

    /// Index into TypeDefs array (only valid for non-builtins).
    #[inline]
    pub fn custom_index(self) -> Option<usize> {
        if self.0 >= TYPE_CUSTOM_START {
            Some((self.0 - TYPE_CUSTOM_START) as usize)
        } else {
            None
        }
    }

    #[inline]
    pub fn from_custom_index(idx: usize) -> Self {
        Self(TYPE_CUSTOM_START + idx as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_id_byte_offset() {
        assert_eq!(StepId(0).byte_offset(), 0);
        assert_eq!(StepId(1).byte_offset(), 8);
        assert_eq!(StepId(10).byte_offset(), 80);
    }

    #[test]
    fn bc_type_id_builtins() {
        assert!(QTypeId::VOID.is_builtin());
        assert!(QTypeId::NODE.is_builtin());
        assert!(QTypeId::STRING.is_builtin());
        assert!(!QTypeId(3).is_builtin());

        assert_eq!(QTypeId::VOID.custom_index(), None);
        assert_eq!(QTypeId(3).custom_index(), Some(0));
        assert_eq!(QTypeId(5).custom_index(), Some(2));
        assert_eq!(QTypeId::from_custom_index(0), QTypeId(3));
    }
}
