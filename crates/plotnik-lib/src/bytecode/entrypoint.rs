//! Entrypoint section types.

use super::instructions::StepAddr;
use super::{StringId, TypeId};

/// Named query definition entry point (8 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Entrypoint {
    /// Definition name.
    pub(crate) name: StringId,
    /// Starting instruction address.
    pub(crate) target: StepAddr,
    /// Result type.
    pub(crate) result_type: TypeId,
    pub(crate) _pad: u16,
}

const _: () = assert!(std::mem::size_of::<Entrypoint>() == 8);

impl Entrypoint {
    /// Create a new entrypoint.
    pub fn new(name: StringId, target: StepAddr, result_type: TypeId) -> Self {
        Self {
            name,
            target,
            result_type,
            _pad: 0,
        }
    }

    /// Decode from 8 bytes (crate-internal deserialization).
    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            name: StringId::new(u16::from_le_bytes([bytes[0], bytes[1]])),
            target: u16::from_le_bytes([bytes[2], bytes[3]]),
            result_type: TypeId(u16::from_le_bytes([bytes[4], bytes[5]])),
            _pad: 0,
        }
    }

    pub fn name(&self) -> StringId {
        self.name
    }
    pub fn target(&self) -> StepAddr {
        self.target
    }
    pub fn result_type(&self) -> TypeId {
        self.result_type
    }
}
