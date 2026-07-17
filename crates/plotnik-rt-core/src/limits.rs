//! Runtime execution limits.
//!
//! A run is bounded by two orthogonal resources: **fuel** (work budget) and
//! **memory** (live runtime heap). Each is governed by a [`Limit`] — `Auto`
//! (sized from the input), `Of(n)` (an explicit ceiling), or `Unbounded` (opt
//! out). A [`RuntimeLimitSpec`] is the policy chosen before a run; resolving it
//! against the source's node count yields the concrete [`ResolvedRuntimeLimits`]
//! the VM enforces.
//!
//! Fuel bounds time-like blowup (catastrophic backtracking); memory bounds
//! space-like blowup (unbounded checkpoint or journal growth). *Call* depth
//! needs no ceiling of its own: backtracking is iterative, so it is pure heap —
//! the frame arena, already part of the memory sum — not a native-stack risk.
//!
//! Generated matchers meter one more resource the VM does not: **decode
//! depth**. The VM materializes output iteratively, but generated typed decoding
//! can recurse through user-visible recursive result types, so recursive
//! decoders enter a [`DecodeDepth`] guard before constructing each nested value.

use std::cell::Cell;

use crate::frame::CallFrameError;

/// A safe run exceeded a configured limit or a fixed runtime capacity.
///
/// The generated matchers' safe entry points report through this; the VM
/// reports the same trips through its own `RuntimeError`. The wording of the
/// two must stay aligned — both describe the same enforcement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LimitExceeded {
    /// The matcher exhausted its fuel before the run finished. The value is
    /// the resolved fuel limit for that run.
    OutOfFuel(u64),
    /// A memory sample found the live runtime heap above the ceiling. `used`
    /// is the measurement at the trip point; because the arenas grow
    /// geometrically it can overshoot `limit` by up to a doubling, so it is
    /// reported alongside the ceiling to make the limit tunable.
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
    /// Resolve to a concrete ceiling, falling back to `auto` for [`Limit::Auto`].
    /// `Unbounded` resolves to `None`.
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
    /// Bound on live runtime heap, in bytes.
    pub memory: Limit,
}

impl Default for RuntimeLimitSpec {
    /// Both resources auto-sized from the input — the safety net is on by default.
    fn default() -> Self {
        Self {
            fuel_limit: Limit::Auto,
            memory: Limit::Auto,
        }
    }
}

impl RuntimeLimitSpec {
    /// Resolve `Auto` limits against the source tree's node count, producing the
    /// concrete ceilings the VM enforces. `source_nodes` is
    /// `tree.root_node().descendant_count()` (O(1) in tree-sitter).
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
    /// Available matcher fuel, or `None` for unbounded.
    pub fuel_limit: Option<u64>,
    /// Maximum live runtime heap in bytes, or `None` for unbounded.
    pub max_memory: Option<u64>,
}

// Both auto ceilings grow linearly with the source node count. A legitimate
// query's work and live state are ~linear in input size, so a linear ceiling
// stays invisible to it while still catching super-linear blowup (catastrophic
// backtracking for fuel, unbounded checkpoint growth for memory). The constants
// are generous headroom over measured legitimate usage, not tight targets.

/// Native stack budget reserved for generated typed decoding.
///
/// This is intentionally lower than a typical main-thread stack: callers can
/// run generated matchers on worker threads, and Rust's default worker stack is
/// commonly 2 MiB.
const DECODE_STACK_BUDGET_BYTES: u64 = 2 * 1024 * 1024;

/// Per-call overhead that the source-level decoder estimate cannot see:
/// return address, saved registers, argument passing, and compiler-chosen
/// temporaries. The emitter supplies the locals it knows about; this padding
/// keeps the formula conservative without requiring backend-specific stack maps.
const DECODE_FRAME_OVERHEAD_BYTES: u64 = 512;

/// Space reserved for one native node handle in generated decoder frames.
///
/// Concrete runtime crates assert that their selected binding's `Node` fits
/// this estimate, so the compiler can use one backend-independent model.
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
