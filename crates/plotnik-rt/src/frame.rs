//! Call frame arena for recursion support.
//!
//! Implements the cactus stack pattern: frames are append-only,
//! with a current pointer that can be restored for backtracking.

/// Which continuation a returning nullable definition selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ReturnOutcome {
    Matched = 0,
    Zero = 1,
}

impl ReturnOutcome {
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Matched),
            1 => Some(Self::Zero),
            _ => None,
        }
    }

    pub const fn to_byte(self) -> u8 {
        self as u8
    }
}

/// Continuations owned by one call frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameReturns {
    matched: u16,
    zero: u16,
}

impl FrameReturns {
    pub fn single(target: u16) -> Self {
        Self {
            matched: target,
            zero: target,
        }
    }

    pub fn split(matched: u16, zero: u16) -> Self {
        Self { matched, zero }
    }

    fn target(self, outcome: ReturnOutcome) -> u16 {
        match outcome {
            ReturnOutcome::Matched => self.matched,
            ReturnOutcome::Zero => self.zero,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// Where to jump for each admitted return outcome.
    pub returns: FrameReturns,
    /// Parent frame index (for cactus stack).
    pub parent: Option<u32>,
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
    pub fn push(&mut self, returns: FrameReturns) -> u32 {
        let idx = self.frames.len() as u32;
        self.frames.push(Frame {
            returns,
            parent: self.current,
        });
        self.current = Some(idx);
        idx
    }

    /// Pop the current frame, returning its return address.
    ///
    /// Panics if the stack is empty.
    pub fn pop(&mut self, outcome: ReturnOutcome) -> u16 {
        let current_idx = self.current.expect("pop on empty frame stack");
        let frame = self.frames[current_idx as usize];
        self.current = frame.parent;
        frame.returns.target(outcome)
    }

    /// Restore frame state for backtracking.
    #[inline]
    pub fn restore(&mut self, frame_index: Option<u32>) {
        self.current = frame_index;
    }

    #[inline]
    pub fn current(&self) -> Option<u32> {
        self.current
    }

    /// Live heap bytes for the append-only frame arena: frame count × frame
    /// size. The arena is never deallocated mid-run (only pruned), so its
    /// `len()` is the true live span.
    #[inline]
    pub fn byte_footprint(&self) -> u64 {
        (self.frames.len() * std::mem::size_of::<Frame>()) as u64
    }

    /// Check if frame stack is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.current.is_none()
    }

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
    /// references them. The `max_frame_idx` is the highest frame index
    /// still referenced by any active checkpoint.
    pub fn prune(&mut self, max_frame_idx: Option<u32>) {
        let keep = match (self.current, max_frame_idx) {
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
