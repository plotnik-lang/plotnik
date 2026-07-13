//! Entry-point section types.

use super::instructions::CodeAddr;
use super::{StringId, TypeId};

/// Named query definition entry point (8 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct EntryPoint {
    /// Definition name.
    name: StringId,
    /// Starting instruction address.
    target: CodeAddr,
    /// Result type.
    result_type: TypeId,
    _pad: u16,
}

const _: () = assert!(std::mem::size_of::<EntryPoint>() == EntryPoint::SIZE);

impl EntryPoint {
    /// Serialized size in bytes.
    pub const SIZE: usize = 8;

    pub fn new(name: StringId, target: CodeAddr, result_type: TypeId) -> Self {
        Self {
            name,
            target,
            result_type,
            _pad: 0,
        }
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            name: StringId::try_from(u16::from_le_bytes([bytes[0], bytes[1]]))
                .expect("entry-point name id must be non-zero"),
            target: CodeAddr::from(u16::from_le_bytes([bytes[2], bytes[3]])),
            result_type: TypeId::from(u16::from_le_bytes([bytes[4], bytes[5]])),
            _pad: 0,
        }
    }

    pub fn name(&self) -> StringId {
        self.name
    }
    pub fn target(&self) -> CodeAddr {
        self.target
    }
    pub fn result_type(&self) -> TypeId {
        self.result_type
    }
}
