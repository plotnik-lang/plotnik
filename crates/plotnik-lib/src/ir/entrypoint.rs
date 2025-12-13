//! Named entrypoints for multi-definition queries.
//!
//! Entrypoints provide named exports for definitions. The default entrypoint
//! is always Transition 0; this table enables accessing other definitions by name.

use super::ids::{StringId, TransitionId, TypeId};

/// Named entrypoint into the query graph.
///
/// Layout: 12 bytes, align 4.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Entrypoint {
    /// String ID for the entrypoint name.
    name_id: StringId,
    _pad: u16,
    /// Target transition (definition entry point).
    target: TransitionId,
    /// Result type of this definition (see ADR-0007).
    result_type: TypeId,
    _pad2: u16,
}

const _: () = assert!(size_of::<Entrypoint>() == 12);
const _: () = assert!(align_of::<Entrypoint>() == 4);

impl Entrypoint {
    /// Creates a new entrypoint.
    pub const fn new(name_id: StringId, target: TransitionId, result_type: TypeId) -> Self {
        Self {
            name_id,
            _pad: 0,
            target,
            result_type,
            _pad2: 0,
        }
    }

    /// Returns the string ID of the entrypoint name.
    #[inline]
    pub const fn name_id(&self) -> StringId {
        self.name_id
    }

    /// Returns the target transition ID.
    #[inline]
    pub const fn target(&self) -> TransitionId {
        self.target
    }

    /// Returns the result type ID.
    #[inline]
    pub const fn result_type(&self) -> TypeId {
        self.result_type
    }
}
