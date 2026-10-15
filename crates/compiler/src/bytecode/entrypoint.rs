//! Entrypoint section types.

use super::instructions::CodeAddr;
use super::{StringId, TypeId};

/// Output effects owned by an entrypoint rather than its shared definition body.
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

/// Identity of a root in the module's entrypoint table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntrypointIdentity {
    Named(StringId),
    Expression,
}

impl EntrypointIdentity {
    pub(crate) fn to_u16(self) -> u16 {
        match self {
            Self::Named(name) => name.into(),
            Self::Expression => 0,
        }
    }
}

/// A module-owned runnable root, whether named or supplied as an expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entrypoint {
    identity: EntrypointIdentity,
    /// Starting instruction address.
    target: CodeAddr,
    /// Result type.
    result_type: TypeId,
    /// Entry-owned result boundary effects.
    boundary: u16,
}

impl Entrypoint {
    pub const SIZE: usize = 8;

    pub(crate) fn new(
        identity: EntrypointIdentity,
        target: CodeAddr,
        result_type: TypeId,
        boundary: EntryBoundary,
    ) -> Self {
        Self {
            identity,
            target,
            result_type,
            boundary: boundary.to_u16(),
        }
    }

    pub fn identity(&self) -> EntrypointIdentity {
        self.identity
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        let identity = match StringId::try_from(u16::from_le_bytes([bytes[0], bytes[1]])) {
            Ok(name) => EntrypointIdentity::Named(name),
            Err(_) => EntrypointIdentity::Expression,
        };
        Self {
            identity,
            target: CodeAddr::from(u16::from_le_bytes([bytes[2], bytes[3]])),
            result_type: TypeId::from(u16::from_le_bytes([bytes[4], bytes[5]])),
            boundary: u16::from_le_bytes([bytes[6], bytes[7]]),
        }
    }

    pub(crate) fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[0..2].copy_from_slice(&self.identity.to_u16().to_le_bytes());
        bytes[2..4].copy_from_slice(&self.target.to_le_bytes());
        bytes[4..6].copy_from_slice(&u16::from(self.result_type).to_le_bytes());
        bytes[6..8].copy_from_slice(&self.boundary.to_le_bytes());
        bytes
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
            .expect("validated entrypoint has a known boundary mode")
    }
}
