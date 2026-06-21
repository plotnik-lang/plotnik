//! Checkpoints for backtracking.
//!
//! When the VM encounters a branch (multiple successors), it saves
//! a checkpoint for each alternative. On failure, it restores the
//! most recent checkpoint and continues.

use std::num::NonZeroU16;

use super::cursor::SkipPolicy;

/// Everything needed to re-enter a callee at the next sibling after a Call's
/// callee fails. Carrying this on the checkpoint (rather than in ambient VM
/// state) keeps the resume fully self-contained: `backtrack` advances the
/// cursor and re-enters the callee without re-running the Call's navigation.
#[derive(Clone, Copy, Debug)]
pub struct CallResume {
    /// Callee entry (raw step index).
    pub(crate) target: u16,
    /// Return address after the callee (raw step index).
    pub(crate) next: u16,
    /// Field constraint the next candidate must satisfy, if any.
    pub(crate) field: Option<NonZeroU16>,
    /// How to advance to the next candidate.
    pub(crate) policy: SkipPolicy,
}

/// The VM state a checkpoint snapshots and later restores: everything shared
/// by both branch and Call-retry checkpoints. Bundling these fields keeps the
/// snapshot at creation and the restore on backtrack in lockstep.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CheckpointState {
    /// Cursor position (tree-sitter descendant_index).
    pub(crate) descendant_index: u32,
    /// Effect stream length at checkpoint.
    pub(crate) effect_watermark: usize,
    /// Frame arena state at checkpoint.
    pub(crate) frame_index: Option<u32>,
    /// Recursion depth at checkpoint.
    pub(crate) recursion_depth: u32,
    /// Suppression depth at checkpoint.
    pub(crate) suppress_depth: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct Checkpoint {
    /// VM state to restore on backtrack.
    pub(crate) state: CheckpointState,
    /// Resume point for a plain (branch) checkpoint (raw step index).
    pub(crate) ip: u16,
    /// If set, this is a Call retry: advance the cursor and re-enter the
    /// callee instead of resuming at `ip`.
    pub(crate) call_resume: Option<CallResume>,
    /// Maximum `frame_index` over this checkpoint and everything beneath it on
    /// the stack. The whole stack's max is therefore the top's `max_frame_idx_below`,
    /// so pruning never has to scan.
    pub(crate) max_frame_idx_below: Option<u32>,
}

#[allow(dead_code)] // Getters useful for debugging/tracing
impl Checkpoint {
    /// Create a plain (branch alternative) checkpoint that resumes at `ip`.
    pub fn branch(state: CheckpointState, ip: u16) -> Self {
        Self {
            state,
            ip,
            call_resume: None,
            max_frame_idx_below: None,
        }
    }

    /// Create a Call retry checkpoint that advances and re-enters the callee.
    /// `call_ip` is the Call's own address, retained only for trace rendering;
    /// re-entry is driven entirely by `call_resume`.
    pub fn call_retry(state: CheckpointState, call_ip: u16, call_resume: CallResume) -> Self {
        Self {
            state,
            ip: call_ip,
            call_resume: Some(call_resume),
            max_frame_idx_below: None,
        }
    }

    pub fn state(&self) -> CheckpointState {
        self.state
    }

    pub fn descendant_index(&self) -> u32 {
        self.state.descendant_index
    }
    pub fn effect_watermark(&self) -> usize {
        self.state.effect_watermark
    }
    pub fn frame_index(&self) -> Option<u32> {
        self.state.frame_index
    }
    pub fn recursion_depth(&self) -> u32 {
        self.state.recursion_depth
    }
    pub fn ip(&self) -> u16 {
        self.ip
    }
    pub fn suppress_depth(&self) -> u16 {
        self.state.suppress_depth
    }
}

/// Stack of checkpoints with O(1) `max_frame_idx` tracking.
///
/// The `max_frame_idx` is maintained for frame arena pruning: we track the
/// highest frame index referenced by any checkpoint so pruning knows which
/// frames are safe to remove. It is kept current through each checkpoint's
/// `max_frame_idx_below` prefix-max rather than recomputed on backtrack.
#[derive(Debug)]
pub struct CheckpointStack {
    stack: Vec<Checkpoint>,
    /// Highest frame index referenced by any checkpoint.
    max_frame_idx: Option<u32>,
}

impl CheckpointStack {
    /// Create an empty checkpoint stack.
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            max_frame_idx: None,
        }
    }

    pub fn push(&mut self, mut checkpoint: Checkpoint) {
        let prev = self.stack.last().and_then(|c| c.max_frame_idx_below);
        checkpoint.max_frame_idx_below = match (checkpoint.state.frame_index, prev) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        self.max_frame_idx = checkpoint.max_frame_idx_below;
        self.stack.push(checkpoint);
    }

    pub fn pop(&mut self) -> Option<Checkpoint> {
        let cp = self.stack.pop()?;
        self.max_frame_idx = self.stack.last().and_then(|c| c.max_frame_idx_below);
        Some(cp)
    }

    /// Get the highest frame index referenced by any checkpoint.
    #[inline]
    pub fn max_frame_idx(&self) -> Option<u32> {
        self.max_frame_idx
    }

    /// Check if empty.
    #[inline]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Get number of checkpoints.
    #[inline]
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Live heap bytes: checkpoint count × checkpoint size.
    #[inline]
    pub fn byte_footprint(&self) -> u64 {
        (self.stack.len() * std::mem::size_of::<Checkpoint>()) as u64
    }
}

impl Default for CheckpointStack {
    fn default() -> Self {
        Self::new()
    }
}
