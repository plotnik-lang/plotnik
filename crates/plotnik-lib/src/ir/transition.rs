//! Transition struct - the fundamental unit of the query IR.
//!
//! Each transition is 64 bytes and cache-line aligned to ensure no transition
//! straddles cache lines. Transitions carry all semantics: matching, effects,
//! and successors. States are implicit junction points.

use super::{EffectOp, Matcher, Nav, RefTransition, Slice, TransitionId};

/// Maximum number of inline successors before spilling to external segment.
pub const MAX_INLINE_SUCCESSORS: usize = 8;

/// A single transition in the query graph.
///
/// Transitions use SSO (small-size optimization) for successors:
/// - 0-8 successors: stored inline in `successor_data`
/// - 9+ successors: `successor_data[0]` is index into successors segment
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct Transition {
    // --- 32 bytes metadata ---
    /// What this transition matches (node kind, wildcard, epsilon).
    pub matcher: Matcher, // 16 bytes

    /// Reference call/return marker for recursive definitions.
    pub ref_marker: RefTransition, // 4 bytes

    /// Number of successor transitions.
    pub successor_count: u32, // 4 bytes

    /// Effects to execute on successful match.
    /// When empty: start_index=0, len=0.
    pub effects: Slice<EffectOp>, // 6 bytes

    /// Navigation instruction (descend/ascend/sibling traversal).
    pub nav: Nav, // 2 bytes

    // --- 32 bytes control flow ---
    /// Successor storage (inline or spilled index).
    ///
    /// - If `successor_count <= 8`: contains `TransitionId` values directly
    /// - If `successor_count > 8`: `successor_data[0]` is index into successors segment
    pub successor_data: [u32; MAX_INLINE_SUCCESSORS], // 32 bytes
}

impl Transition {
    /// Returns `true` if successors are stored inline.
    #[inline]
    pub fn has_inline_successors(&self) -> bool {
        self.successor_count as usize <= MAX_INLINE_SUCCESSORS
    }

    /// Returns inline successors if they fit, `None` if spilled.
    #[inline]
    pub fn inline_successors(&self) -> Option<&[TransitionId]> {
        if self.has_inline_successors() {
            Some(&self.successor_data[..self.successor_count as usize])
        } else {
            None
        }
    }

    /// Returns the spilled successor segment index and count.
    /// Panics if successors are inline.
    #[inline]
    pub fn spilled_successors_index(&self) -> u32 {
        debug_assert!(
            !self.has_inline_successors(),
            "successors are inline, not spilled"
        );
        self.successor_data[0]
    }
}

// Compile-time size/alignment verification
const _: () = {
    assert!(core::mem::size_of::<Transition>() == 64);
    assert!(core::mem::align_of::<Transition>() == 64);
};
