use std::cell::Cell;

use crate::frame::CallFrameError;

/// A safe run exceeded a configured limit or a fixed runtime capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LimitExceeded {
    /// The matcher exhausted its fuel before the run finished. The value is
    /// the resolved fuel limit for that run.
    OutOfFuel(u64),
    /// A memory sample exceeded the ceiling. `used` counts occupied arena
    /// entries and estimated cursor storage, not allocated heap bytes.
    /// Sampling can overshoot `limit` without a fixed bound.
    Memory { used: u64, limit: u64 },
    /// The committed value's nesting exceeded the decode-depth ceiling.
    /// Reported only by generated matchers (the VM renders output
    /// iteratively and has no such ceiling): their typed decoder recurses,
    /// so the metered path refuses the match before decoding could exhaust
    /// the native stack. Raise the module's `depth` policy — and run with a
    /// stack to match — if values this deep are expected.
    DecodeDepth(u64),
    /// The source-driven call stack exceeded a fixed-width runtime capacity.
    CallFrame(CallFrameError),
}

impl std::fmt::Display for LimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LimitExceeded::OutOfFuel(limit) => {
                write!(f, "exhausted the fuel limit of {limit}")
            }
            LimitExceeded::Memory { used, limit } => {
                write!(
                    f,
                    "exceeded the memory limit of {limit} bytes (used {used} bytes)"
                )
            }
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

/// One resource's limit policy, independent of any particular input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Limit {
    /// Derive a ceiling from the input size (see [`RuntimeLimitSpec::resolve`]).
    Auto,
    /// An explicit ceiling.
    Of(u64),
    /// No ceiling — opt out of the safety net.
    Unbounded,
}

impl Limit {
    fn resolve(self, auto: u64) -> Option<u64> {
        match self {
            Limit::Auto => Some(auto),
            Limit::Of(n) => Some(n),
            Limit::Unbounded => None,
        }
    }
}

/// The limit policy for a run, before it is sized to a specific input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RuntimeLimitSpec {
    /// Matcher work budget, in fuel units.
    pub fuel_limit: Limit,
    /// Sampled memory-accounting ceiling in bytes, not a hard allocation limit.
    pub memory: Limit,
}

impl Default for RuntimeLimitSpec {
    /// Defaults to `Auto` for both fuel and memory.
    fn default() -> Self {
        Self {
            fuel_limit: Limit::Auto,
            memory: Limit::Auto,
        }
    }
}

impl RuntimeLimitSpec {
    /// Resolve `Auto` limits once against the operation scope's node count.
    /// `source_nodes` is `scope.descendant_count()` (O(1) in Tree-sitter).
    /// Traversal and all candidate applications share the resulting fuel.
    pub fn resolve(self, source_nodes: u32) -> ResolvedRuntimeLimits {
        ResolvedRuntimeLimits {
            fuel_limit: self.fuel_limit.resolve(auto_fuel(source_nodes)),
            max_memory: self.memory.resolve(auto_memory(source_nodes)),
        }
    }
}

/// The concrete per-resource ceilings for one run. `None` means unbounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedRuntimeLimits {
    /// Available operation fuel, or `None` for unbounded.
    pub fuel_limit: Option<u64>,
    /// Sampled memory-accounting ceiling in bytes, or `None` for unbounded.
    pub max_memory: Option<u64>,
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

const FUEL_BASE: u64 = 1_000_000;
const FUEL_PER_NODE: u64 = 1_024;

const MEMORY_BASE: u64 = 64 * 1024 * 1024;
const MEMORY_PER_NODE: u64 = 256;

fn auto_fuel(source_nodes: u32) -> u64 {
    FUEL_BASE.saturating_add(FUEL_PER_NODE.saturating_mul(source_nodes as u64))
}

fn auto_memory(source_nodes: u32) -> u64 {
    MEMORY_BASE.saturating_add(MEMORY_PER_NODE.saturating_mul(source_nodes as u64))
}
