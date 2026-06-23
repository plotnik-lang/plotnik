//! Entrypoint section types.

use super::instructions::StepAddr;
use super::{StringId, TypeId};

/// Named query definition entry point (8 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Entrypoint {
    /// Definition name.
    name: StringId,
    /// Starting instruction address.
    target: StepAddr,
    /// Result type.
    result_type: TypeId,
    _pad: u16,
}

const _: () = assert!(std::mem::size_of::<Entrypoint>() == Entrypoint::SIZE);

impl Entrypoint {
    /// Serialized size in bytes.
    pub const SIZE: usize = 8;

    pub fn new(name: StringId, target: StepAddr, result_type: TypeId) -> Self {
        Self {
            name,
            target,
            result_type,
            _pad: 0,
        }
    }

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
