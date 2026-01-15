//! Call frame arena for recursion support.
//!
//! Implements the cactus stack pattern: frames are append-only,
//! with a current pointer that can be restored for backtracking.

/// Call frame for recursion support.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// Where to jump on Return (raw step index).
    pub return_addr: u16,
    /// Parent frame index (for cactus stack).
    pub parent: Option<u32>,
    /// Tree depth at Call time (for level restoration on Return).
    pub saved_depth: u32,
}

/// Append-only arena for frames (cactus stack implementation).
///
/// Frames are never deallocated during execution - "pop" just moves
/// the current pointer. This allows checkpoint restoration without
/// invalidating frames referenced by other checkpoints.
#[derive(Debug)]
pub struct FrameArena {
    frames: Vec<Frame>,
    current: Option<u32>,
}

impl FrameArena {
    /// Create an empty frame arena.
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            current: None,
        }
    }

    /// Push a new frame, returns its index.
    pub fn push(&mut self, return_addr: u16, saved_depth: u32) -> u32 {
        let idx = self.frames.len() as u32;
        self.frames.push(Frame {
            return_addr,
            parent: self.current,
            saved_depth,
        });
        self.current = Some(idx);
        idx
    }

    /// Pop the current frame, returning its return address and saved depth.
    ///
    /// Panics if the stack is empty.
    pub fn pop(&mut self) -> (u16, u32) {
        let current_idx = self.current.expect("pop on empty frame stack");
        let frame = self.frames[current_idx as usize];
        self.current = frame.parent;
        (frame.return_addr, frame.saved_depth)
    }

    /// Restore frame state for backtracking.
    #[inline]
    pub fn restore(&mut self, frame_index: Option<u32>) {
        self.current = frame_index;
    }

    /// Get current frame index.
    #[inline]
    pub fn current(&self) -> Option<u32> {
        self.current
    }

    /// Check if frame stack is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.current.is_none()
    }

    /// Get current call depth.
    #[allow(dead_code)]
    pub fn depth(&self) -> u32 {
        let mut depth = 0;
        let mut idx = self.current;
        while let Some(i) = idx {
            depth += 1;
            idx = self.frames[i as usize].parent;
        }
        depth
    }

    /// Prune frames above high-water mark.
    ///
    /// Frames are only pruned after Return, when we know no checkpoint
    /// references them. The `max_referenced` is the highest frame index
    /// still referenced by any active checkpoint.
    pub fn prune(&mut self, max_referenced: Option<u32>) {
        // Keep frames up to max(current, max_referenced)
        let keep = match (self.current, max_referenced) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };

        if let Some(high_water) = keep {
            self.frames.truncate(high_water as usize + 1);
        }
    }
}

impl Default for FrameArena {
    fn default() -> Self {
        Self::new()
    }
}
