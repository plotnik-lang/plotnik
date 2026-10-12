//! Entry-point section types.

use super::instructions::CodeAddr;
use super::{StringId, TypeId};

/// Output effects owned by an entry point rather than its shared definition body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum EntryBoundary {
    /// The definition body already produces the complete result.
    Passthrough = 0,
    /// Capture the current node after the definition returns successfully.
    Node = 1,
    /// Open a root record before execution and close it after a successful return.
    Record = 2,
}

impl EntryBoundary {
    pub(crate) fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::Passthrough),
            1 => Some(Self::Node),
            2 => Some(Self::Record),
            _ => None,
        }
    }

    pub(crate) fn to_u16(self) -> u16 {
        self as u16
    }
}

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
    /// Entry-owned result boundary effects.
    boundary: u16,
}

const _: () = assert!(std::mem::size_of::<EntryPoint>() == EntryPoint::SIZE);

impl EntryPoint {
    /// Serialized size in bytes.
    pub const SIZE: usize = 8;

    pub(crate) fn new(
        name: StringId,
        target: CodeAddr,
        result_type: TypeId,
        boundary: EntryBoundary,
    ) -> Self {
        Self {
            name,
            target,
            result_type,
            boundary: boundary.to_u16(),
        }
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            name: StringId::try_from(u16::from_le_bytes([bytes[0], bytes[1]]))
                .expect("entry point name id must be non-zero"),
            target: CodeAddr::from(u16::from_le_bytes([bytes[2], bytes[3]])),
            result_type: TypeId::from(u16::from_le_bytes([bytes[4], bytes[5]])),
            boundary: u16::from_le_bytes([bytes[6], bytes[7]]),
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

    pub(crate) fn try_boundary(&self) -> Option<EntryBoundary> {
        EntryBoundary::from_u16(self.boundary)
    }

    pub(crate) fn boundary(&self) -> EntryBoundary {
        self.try_boundary()
            .expect("validated entry point has a known boundary mode")
    }
}
