use std::cell::Cell;

use crate::frame::CallFrameError;

/// A safe run exceeded a configured limit or a fixed runtime capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LimitExceeded {
    /// The committed value's nesting exceeded the decode-depth ceiling.
    /// Reported only by generated matchers (the VM renders output
    /// iteratively and has no such ceiling): their typed decoder recurses,
    /// so decoding refuses the match before it could exhaust
    /// the native stack. Raise the module's `depth` policy — and run with a
    /// stack to match — if values this deep are expected.
    DecodeDepth(u64),
    /// The source-driven call stack exceeded a fixed-width runtime capacity.
    CallFrame(CallFrameError),
}

impl std::fmt::Display for LimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LimitExceeded::DecodeDepth(max) => {
                write!(f, "exceeded the decode depth limit of {max} nested values")
            }
            LimitExceeded::CallFrame(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for LimitExceeded {}

impl From<CallFrameError> for LimitExceeded {
    fn from(error: CallFrameError) -> Self {
        Self::CallFrame(error)
    }
}

pub struct DecodeDepth {
    max: Option<u64>,
    current: Cell<u64>,
}

impl DecodeDepth {
    pub fn new(max: Option<u64>) -> Self {
        Self {
            max,
            current: Cell::new(0),
        }
    }

    pub fn enter(&self) -> Result<DecodeDepthGuard<'_>, LimitExceeded> {
        let current = self.current.get();
        let next = current.checked_add(1).unwrap_or_else(|| {
            panic!(
                "result decoding depth overflowed u64 while entering a nested value: \
                 current_depth={current}"
            )
        });
        if let Some(max) = self.max
            && next > max
        {
            return Err(LimitExceeded::DecodeDepth(max));
        }
        self.current.set(next);
        Ok(DecodeDepthGuard { depth: self })
    }
}

pub struct DecodeDepthGuard<'a> {
    depth: &'a DecodeDepth,
}

impl Drop for DecodeDepthGuard<'_> {
    fn drop(&mut self) {
        let current = self.depth.current.get();
        self.depth
            .current
            .set(current.checked_sub(1).expect("decode depth underflow"));
    }
}

/// The depth policy for generated result decoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Limit {
    /// Derive a ceiling from the generated decoder's frame size.
    Auto,
    /// An explicit ceiling.
    Of(u64),
    /// No ceiling — opt out of the safety net.
    Unbounded,
}

const DECODE_STACK_BUDGET_BYTES: u64 = 2 * 1024 * 1024;

const DECODE_FRAME_OVERHEAD_BYTES: u64 = 512;

/// Space reserved for one native node handle in generated decoder frames.
///
/// Execution builds assert that `tree_sitter::Node` fits this estimate, allowing
/// the compiler to estimate frames without linking Tree-sitter.
#[doc(hidden)]
pub const GENERATED_NODE_VALUE_BYTES: u64 = 48;

/// Compute the default decode-depth ceiling for a generated matcher module.
///
/// The emitter passes its conservative maximum decoder-frame estimate. The
/// ceiling then scales down for wide readers and up for narrow readers while
/// staying tied to the native-stack budget this limit protects.
pub const fn decode_depth_auto(decoder_frame_bytes: u64) -> u64 {
    let frame_bytes = decoder_frame_bytes.saturating_add(DECODE_FRAME_OVERHEAD_BYTES);
    let depth = DECODE_STACK_BUDGET_BYTES / frame_bytes;
    if depth == 0 { 1 } else { depth }
}
