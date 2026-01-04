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
    pub descendant_index: u32,
    /// Effect stream length at checkpoint.
    pub effect_watermark: usize,
    /// Frame arena state at checkpoint.
    pub frame_index: Option<u32>,
    /// Recursion depth at checkpoint.
    pub recursion_depth: u32,
    /// Resume point (raw step index).
    pub ip: u16,
    /// If set, advance cursor before retrying (for Call instruction retry).
    /// When a Call navigates and the callee fails, we need to try the next
    /// sibling. This policy determines how to advance.
    pub skip_policy: Option<SkipPolicy>,
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
