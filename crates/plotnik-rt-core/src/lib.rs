//! Backend-independent runtime primitives shared by Plotnik's concrete
//! Tree-sitter runtimes and compiler.

/// ABI implemented by the concrete runtime crates and required by generated modules.
pub const RUNTIME_ABI: u32 = 5;

mod checkpoint;
mod dfa;
mod frame;
mod ids;
mod limits;
mod nav;
mod node_class;

pub use checkpoint::{
    CallResume, Checkpoint, CheckpointStack, CheckpointState, EffectDepths, Resume,
};
pub use dfa::{RegexDfas, StaticDfa, deserialize_dfa};
pub use frame::{Frame, FrameArena, PortId};
pub use ids::{NodeFieldId, NodeKindId, ZeroIdError};
pub use limits::{
    DecodeDepth, GENERATED_NODE_VALUE_BYTES, Limit, LimitExceeded, ResolvedRuntimeLimits,
    RuntimeLimitSpec, decode_depth_auto,
};
pub use nav::{Nav, SkipPolicy};
pub use node_class::{NodeClass, SkipClass};
