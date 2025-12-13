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
///
/// Layout (64 bytes total, 64-byte aligned):
/// ```text
/// offset 0:  matcher (16 bytes)
/// offset 16: ref_marker (4 bytes)
/// offset 20: nav (2 bytes)
/// offset 22: effects_len (2 bytes)
/// offset 24: successor_count (4 bytes)
/// offset 28: effects_start (4 bytes)
/// offset 32: successor_data (32 bytes)
/// ```
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct Transition {
    // --- 32 bytes metadata ---
    /// What this transition matches (node kind, wildcard, epsilon).
    pub matcher: Matcher, // 16 bytes, offset 0

    /// Reference call/return marker for recursive definitions.
    pub ref_marker: RefTransition, // 4 bytes, offset 16

    /// Navigation instruction (descend/ascend/sibling traversal).
    pub nav: Nav, // 2 bytes, offset 20

    /// Number of effect operations (inlined from Slice for alignment).
    effects_len: u16, // 2 bytes, offset 22

    /// Number of successor transitions.
    pub successor_count: u32, // 4 bytes, offset 24

    /// Start index into effects segment (inlined from Slice for alignment).
    effects_start: u32, // 4 bytes, offset 28

    // --- 32 bytes control flow ---
    /// Successor storage (inline or spilled index).
    ///
    /// - If `successor_count <= 8`: contains `TransitionId` values directly
    /// - If `successor_count > 8`: `successor_data[0]` is index into successors segment
    pub successor_data: [u32; MAX_INLINE_SUCCESSORS], // 32 bytes, offset 32
}

impl Transition {
    /// Creates a new transition with all fields.
    #[inline]
    pub fn new(
        matcher: Matcher,
        ref_marker: RefTransition,
        nav: Nav,
        effects: Slice<EffectOp>,
        successor_count: u32,
        successor_data: [u32; MAX_INLINE_SUCCESSORS],
    ) -> Self {
        Self {
            matcher,
            ref_marker,
            nav,
            effects_len: effects.len(),
            successor_count,
            effects_start: effects.start_index(),
            successor_data,
        }
    }

    /// Returns the effects slice.
    #[inline]
    pub fn effects(&self) -> Slice<EffectOp> {
        Slice::new(self.effects_start, self.effects_len)
    }

    /// Sets the effects slice.
    #[inline]
    pub fn set_effects(&mut self, effects: Slice<EffectOp>) {
        self.effects_start = effects.start_index();
        self.effects_len = effects.len();
    }

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
