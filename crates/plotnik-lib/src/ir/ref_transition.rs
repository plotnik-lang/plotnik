//! Definition call/return markers for recursive transition network.
//!
//! See ADR-0005 for semantics of Enter/Exit transitions.

use super::RefId;

/// Marks a transition as entering or exiting a definition reference.
///
/// A transition can hold at most one `RefTransition`. Sequences like
/// `Enter(A) → Enter(B)` require epsilon chains.
///
/// Layout: 1-byte discriminant + 1-byte padding + 2-byte RefId = 4 bytes, align 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, u8)]
pub enum RefTransition {
    /// No definition boundary crossing.
    None,

    /// Push call frame with return transitions.
    ///
    /// For `Enter(ref_id)` transitions, successors have special structure:
    /// - `successors()[0]`: definition entry point (where to jump)
    /// - `successors()[1..]`: return transitions (stored in call frame)
    Enter(RefId),

    /// Pop frame, continue with stored return transitions.
    ///
    /// Successors are ignored—returns come from the call frame pushed at `Enter`.
    Exit(RefId),
}

impl RefTransition {
    /// Returns `true` if this is `None`.
    #[inline]
    pub fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    /// Returns the ref ID if this is `Enter` or `Exit`.
    #[inline]
    pub fn ref_id(self) -> Option<RefId> {
        match self {
            Self::None => None,
            Self::Enter(id) | Self::Exit(id) => Some(id),
        }
    }
}

impl Default for RefTransition {
    fn default() -> Self {
        Self::None
    }
}
