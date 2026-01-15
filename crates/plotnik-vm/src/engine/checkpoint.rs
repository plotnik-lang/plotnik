//! Checkpoints for backtracking.
//!
//! When the VM encounters a branch (multiple successors), it saves
//! a checkpoint for each alternative. On failure, it restores the
//! most recent checkpoint and continues.

use super::cursor::SkipPolicy;

/// Checkpoint for backtracking.
#[derive(Clone, Copy, Debug)]
pub struct Checkpoint {
    /// Cursor position (tree-sitter descendant_index).
    pub(crate) descendant_index: u32,
    /// Effect stream length at checkpoint.
    pub(crate) effect_watermark: usize,
    /// Frame arena state at checkpoint.
    pub(crate) frame_index: Option<u32>,
    /// Recursion depth at checkpoint.
    pub(crate) recursion_depth: u32,
    /// Resume point (raw step index).
    pub(crate) ip: u16,
    /// If set, advance cursor before retrying (for Call instruction retry).
    /// When a Call navigates and the callee fails, we need to try the next
    /// sibling. This policy determines how to advance.
    pub(crate) skip_policy: Option<SkipPolicy>,
    /// Suppression depth at checkpoint.
    pub(crate) suppress_depth: u16,
}

#[allow(dead_code)] // Getters useful for debugging/tracing
impl Checkpoint {
    /// Create a new checkpoint.
    pub fn new(
        descendant_index: u32,
        effect_watermark: usize,
        frame_index: Option<u32>,
        recursion_depth: u32,
        ip: u16,
        skip_policy: Option<SkipPolicy>,
        suppress_depth: u16,
    ) -> Self {
        Self {
            descendant_index,
            effect_watermark,
            frame_index,
            recursion_depth,
            ip,
            skip_policy,
            suppress_depth,
        }
    }

    pub fn descendant_index(&self) -> u32 {
        self.descendant_index
    }
    pub fn effect_watermark(&self) -> usize {
        self.effect_watermark
    }
    pub fn frame_index(&self) -> Option<u32> {
        self.frame_index
    }
    pub fn recursion_depth(&self) -> u32 {
        self.recursion_depth
    }
    pub fn ip(&self) -> u16 {
        self.ip
    }
    pub fn skip_policy(&self) -> Option<SkipPolicy> {
        self.skip_policy
    }
    pub fn suppress_depth(&self) -> u16 {
        self.suppress_depth
    }
}

/// Stack of checkpoints with O(1) max_frame_ref tracking.
///
/// The `max_frame_ref` is maintained for frame arena pruning:
/// we track the highest frame index referenced by any checkpoint
/// so pruning knows which frames are safe to remove.
#[derive(Debug)]
pub struct CheckpointStack {
    stack: Vec<Checkpoint>,
    /// Highest frame index referenced by any checkpoint.
    max_frame_ref: Option<u32>,
}

impl CheckpointStack {
    /// Create an empty checkpoint stack.
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            max_frame_ref: None,
        }
    }

    /// Push a checkpoint.
    pub fn push(&mut self, checkpoint: Checkpoint) {
        // Update max_frame_ref (O(1))
        if let Some(frame_idx) = checkpoint.frame_index {
            self.max_frame_ref = Some(match self.max_frame_ref {
                Some(max) => max.max(frame_idx),
                None => frame_idx,
            });
        }
        self.stack.push(checkpoint);
    }

    /// Pop and return the most recent checkpoint.
    pub fn pop(&mut self) -> Option<Checkpoint> {
        let cp = self.stack.pop()?;

        // Recompute max_frame_ref only if we removed the max holder
        // This is O(1) amortized: each checkpoint contributes to at most
        // one recomputation over its lifetime.
        if cp.frame_index == self.max_frame_ref && !self.stack.is_empty() {
            self.max_frame_ref = self.stack.iter().filter_map(|c| c.frame_index).max();
        } else if self.stack.is_empty() {
            self.max_frame_ref = None;
        }

        Some(cp)
    }

    /// Get the highest frame index referenced by any checkpoint.
    #[inline]
    pub fn max_frame_ref(&self) -> Option<u32> {
        self.max_frame_ref
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
}

impl Default for CheckpointStack {
    fn default() -> Self {
        Self::new()
    }
}
