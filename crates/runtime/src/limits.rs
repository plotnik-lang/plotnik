use crate::frame::CallFrameError;

/// A safe run exceeded a configured limit or a fixed runtime capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LimitExceeded {
    /// The operation exhausted its fuel. The value is the resolved fuel limit.
    OutOfFuel(u64),
    /// Admitting more owned capacity would exceed the memory limit. `used`
    /// is the charge the refused request would have reached, in bytes.
    Memory { used: u64, limit: u64 },
    /// The allocator refused a block of `bytes`, or the requested capacity
    /// does not fit the address space.
    Allocation { bytes: u64 },
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
                    "exceeded the memory limit of {limit} bytes (needed {used} bytes)"
                )
            }
            LimitExceeded::Allocation { bytes } => {
                write!(f, "failed to allocate {bytes} bytes")
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
    /// Ceiling on owned capacity, in bytes.
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
    /// Ceiling on owned capacity in bytes, or `None` for unbounded.
    pub max_memory: Option<u64>,
}

/// Space reserved for one native node handle in generated size estimates.
///
/// Execution builds assert that `tree_sitter::Node` fits this estimate, allowing
/// the compiler to estimate generated value sizes without linking Tree-sitter.
#[doc(hidden)]
pub const GENERATED_NODE_VALUE_BYTES: u64 = 48;

/// Largest by-value generated result type, in bytes.
///
/// Result construction never recurses on the shape of the input, but each
/// generated value is still built and moved by value: its constructor, the
/// typed construction stacks, and the root moving through `Match`, `Option`
/// and `Result`. Unoptimized builds keep several copies in one frame.
/// Bounding every generated type keeps native stack use a small constant
/// multiple of this cap, independent of source and result depth. 8 KiB fits
/// more than 150 node captures in one record and leaves wide margin on a
/// 256 KiB thread stack in debug builds.
pub const GENERATED_VALUE_CAP: usize = 8 * 1024;

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
