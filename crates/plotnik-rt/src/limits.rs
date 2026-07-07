//! Runtime execution limits.
//!
//! A run is bounded by two orthogonal resources: **steps** (work performed) and
//! **memory** (live runtime heap). Each is governed by a [`Limit`] — `Auto`
//! (sized from the input), `Of(n)` (an explicit ceiling), or `Unbounded` (opt
//! out). A [`RuntimeLimitSpec`] is the policy chosen before a run; resolving it
//! against the source's node count yields the concrete [`ResolvedRuntimeLimits`]
//! the VM enforces.
//!
//! Steps bound time-like blowup (catastrophic backtracking); memory bounds
//! space-like blowup (unbounded checkpoint or effect growth). *Call* depth
//! needs no ceiling of its own: backtracking is iterative, so it is pure heap —
//! the frame arena, already part of the memory sum — not a native-stack risk.
//!
//! Generated matchers meter one more resource the VM does not: **replay
//! depth**, the nesting of the committed value (`Engine::effect_depth`,
//! ceiling [`REPLAY_DEPTH_AUTO`]). The VM materializes output iteratively, but
//! the generated typed replay recurses — one native frame per nested value —
//! so its metered entry points refuse a match that nests past the compiled-in
//! bound *before* replay puts the native stack at risk. Unmetered entry points
//! skip this check like every other: they are the caller's declaration that
//! the input is trusted.

/// A metered run exceeded one of its resolved ceilings.
///
/// The generated matchers' `try_*` entry points report through this; the VM
/// reports the same trips through its own `RuntimeError` (which folds in
/// interpretation-only failures like module errors). The wording of the two
/// must stay aligned — both describe the same enforcement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitError {
    /// The step ceiling was reached before the run finished.
    Steps(u64),
    /// A memory sample found the live runtime heap above the ceiling. `used`
    /// is the measurement at the trip point; because the arenas grow
    /// geometrically it can overshoot `limit` by up to a doubling, so it is
    /// reported alongside the ceiling to make the limit tunable.
    Memory { used: u64, limit: u64 },
    /// The committed value's nesting exceeded the replay-depth ceiling.
    /// Reported only by generated matchers (the VM renders output
    /// iteratively and has no such ceiling): their typed replay recurses,
    /// so the metered path refuses the match before replay could exhaust
    /// the native stack. Raise the module's `depth` policy — and run with a
    /// stack to match — if values this deep are expected.
    Depth(u64),
}

impl std::fmt::Display for LimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LimitError::Steps(max) => {
                write!(f, "exceeded the step limit of {max} steps")
            }
            LimitError::Memory { used, limit } => {
                write!(
                    f,
                    "exceeded the memory limit of {limit} bytes (used {used} bytes)"
                )
            }
            LimitError::Depth(max) => {
                write!(f, "exceeded the replay depth limit of {max} nested values")
            }
        }
    }
}

impl std::error::Error for LimitError {}

/// One resource's limit policy, independent of any particular input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeLimitSpec {
    /// Bound on total work (instruction dispatches).
    pub steps: Limit,
    /// Bound on live runtime heap, in bytes.
    pub memory: Limit,
}

impl Default for RuntimeLimitSpec {
    /// Both resources auto-sized from the input — the safety net is on by default.
    fn default() -> Self {
        Self {
            steps: Limit::Auto,
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
            max_steps: self.steps.resolve(auto_steps(source_nodes)),
            max_memory: self.memory.resolve(auto_memory(source_nodes)),
        }
    }
}

/// The concrete per-resource ceilings for one run. `None` means unbounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedRuntimeLimits {
    /// Maximum instruction dispatches, or `None` for unbounded.
    pub max_steps: Option<u64>,
    /// Maximum live runtime heap in bytes, or `None` for unbounded.
    pub max_memory: Option<u64>,
}

// Both auto ceilings grow linearly with the source node count. A legitimate
// query's work and live state are ~linear in input size, so a linear ceiling
// stays invisible to it while still catching super-linear blowup (catastrophic
// backtracking for steps, unbounded checkpoint growth for memory). The constants
// are generous headroom over measured legitimate usage, not tight targets.

/// The `Auto` replay-depth ceiling generated matchers compile in. Flat, not
/// input-scaled: it guards the *native stack*, a per-process resource that
/// does not grow with the input. 1024 nested values times a generous
/// per-reader frame estimate stays comfortably inside even a 2 MiB worker
/// stack, while real code rarely nests output past a few hundred levels.
pub const REPLAY_DEPTH_AUTO: u64 = 1024;

const STEPS_BASE: u64 = 1_000_000;
const STEPS_PER_NODE: u64 = 1_024;

const MEMORY_BASE: u64 = 64 * 1024 * 1024;
const MEMORY_PER_NODE: u64 = 256;

fn auto_steps(source_nodes: u32) -> u64 {
    STEPS_BASE.saturating_add(STEPS_PER_NODE.saturating_mul(source_nodes as u64))
}

fn auto_memory(source_nodes: u32) -> u64 {
    MEMORY_BASE.saturating_add(MEMORY_PER_NODE.saturating_mul(source_nodes as u64))
}
