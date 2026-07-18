//! Call frame arena for recursion support.
//!
//! Implements the cactus stack pattern: frames are append-only,
//! with a current pointer that can be restored for backtracking.

/// Dense callee-local return port.
///
/// Generalized callees expose only the ports they can reach, numbered from
/// zero. The semantic exit universe currently has eight members, so every
/// runtime port fits in three bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PortId(u8);

impl PortId {
    /// Maximum number of return ports exposed by one callee.
    pub const COUNT: u8 = 8;
    pub const ZERO: Self = Self(0);

    /// Construct a port when `index` is in the runtime port universe.
    pub const fn new(index: u8) -> Option<Self> {
        if index < Self::COUNT {
            Some(Self(index))
        } else {
            None
        }
    }

    /// Decode a port from its byte representation.
    pub const fn from_byte(byte: u8) -> Option<Self> {
        Self::new(byte)
    }

    /// Construct a port for generated code, rejecting invalid constants during
    /// const evaluation.
    pub const fn from_raw(index: u8) -> Self {
        match Self::new(index) {
            Some(port) => port,
            None => panic!("return port must be less than 8"),
        }
    }

    pub const fn to_byte(self) -> u8 {
        self.0
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// This port's position in a per-port bit mask.
    pub const fn bit(self) -> u8 {
        1 << self.0
    }

    /// Mask containing every dense port from zero through `port_count - 1`.
    pub const fn dense_mask(port_count: usize) -> u8 {
        assert!(
            port_count <= Self::COUNT as usize,
            "port count must be at most 8"
        );
        ((1u16 << port_count) - 1) as u8
    }
}

const _: () = assert!(
    PortId::COUNT == u8::BITS as u8,
    "`PortId::COUNT` must equal `u8::BITS` so the return-port universe exactly fills its wire mask"
);

impl From<PortId> for u8 {
    fn from(port: PortId) -> Self {
        port.to_byte()
    }
}

impl From<PortId> for usize {
    fn from(port: PortId) -> Self {
        port.index()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// Immutable executor-owned token identifying the call's return map.
    pub call_site: u16,
    /// Parent frame index (for cactus stack).
    pub parent: Option<u32>,
}

/// A source-driven call could not be represented by the runtime's frame state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallFrameError {
    ArenaCapacity {
        call_site: u16,
        existing_frames: usize,
    },
    RecursionDepth {
        call_site: u16,
        current_depth: u32,
    },
}

impl std::fmt::Display for CallFrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ArenaCapacity {
                call_site,
                existing_frames,
            } => write!(
                f,
                "frame arena cannot allocate call site {call_site}: {existing_frames} existing \
                 frames exceed the u32 index space"
            ),
            Self::RecursionDepth {
                call_site,
                current_depth,
            } => write!(
                f,
                "runtime recursion depth overflowed u32 while entering call site {call_site}: \
                 current_depth={current_depth}"
            ),
        }
    }
}

impl std::error::Error for CallFrameError {}

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
    pub fn push(&mut self, call_site: u16) -> Result<u32, CallFrameError> {
        let idx = u32::try_from(self.frames.len()).map_err(|_| CallFrameError::ArenaCapacity {
            call_site,
            existing_frames: self.frames.len(),
        })?;
        self.frames.push(Frame {
            call_site,
            parent: self.current,
        });
        self.current = Some(idx);
        Ok(idx)
    }

    /// Pop the current frame, returning its executor-owned call-site token.
    ///
    /// Panics if the stack is empty.
    pub fn pop(&mut self) -> u16 {
        let current_idx = self.current.expect("pop on empty frame stack");
        let frame = self.frames[current_idx as usize];
        self.current = frame.parent;
        frame.call_site
    }

    /// Restore frame state for backtracking.
    #[inline]
    pub fn restore(&mut self, frame_index: Option<u32>) {
        if let Some(index) = frame_index.filter(|&index| index as usize >= self.frames.len()) {
            panic!(
                "backtracking tried to restore frame index {index}, but the arena contains only {} \
                 frames",
                self.frames.len()
            );
        }
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
            if high_water as usize >= self.frames.len() {
                panic!(
                    "frame pruning computed high-water index {high_water}, but the arena contains \
                     only {} frames: current={:?}, checkpoint_max={max_frame_idx:?}",
                    self.frames.len(),
                    self.current
                );
            }
            self.frames.truncate(high_water as usize + 1);
        } else {
            self.frames.clear();
        }
    }
}

impl Default for FrameArena {
    fn default() -> Self {
        Self::new()
    }
}
