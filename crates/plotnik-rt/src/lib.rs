//! Shared runtime engine for Plotnik queries.
//!
//! The compiler and executors share instruction types, backtracking state,
//! regex automata, and execution limits. The default `execution` feature adds
//! Tree-sitter navigation, match journals, and typed result decoding. Compiler-only
//! builds disable it to avoid linking Tree-sitter's C runtime.

/// Current interface required by generated Rust query modules.
pub const RUNTIME_ABI: u32 = 0;

mod checkpoint;
mod dfa;
mod frame;
mod ids;
mod limits;
mod nav;
mod node_class;

#[cfg(feature = "debug")]
pub mod debug;

#[cfg(feature = "execution")]
mod cursor;
#[cfg(feature = "execution")]
mod engine;
#[cfg(feature = "execution")]
mod journal;
#[cfg(feature = "execution")]
mod result_decoder;
#[cfg(feature = "serde")]
mod serialize;
#[cfg(feature = "execution")]
mod surface;

pub use checkpoint::{
    CallResume, Checkpoint, CheckpointStack, CheckpointState, EffectDepths, Resume,
};
pub use dfa::{RegexDfas, StaticDfa, deserialize_dfa};
pub use frame::{CallFrameError, Frame, FrameArena, PortId};
pub use ids::{InvalidNodeKindId, NodeFieldId, NodeKindId, ZeroIdError};
pub use limits::{
    DecodeDepth, GENERATED_NODE_VALUE_BYTES, Limit, LimitExceeded, ResolvedRuntimeLimits,
    RuntimeLimitSpec, decode_depth_auto,
};
pub use nav::{Nav, SkipPolicy};
pub use node_class::{NodeClass, SkipClass};

#[cfg(feature = "execution")]
pub use cursor::CursorWrapper;
#[cfg(feature = "execution")]
pub use engine::Engine;
#[cfg(feature = "execution")]
pub use journal::{JournalEvent, MatchJournal, OutputEvents, node_text, source_text};
#[cfg(feature = "execution")]
pub use result_decoder::ResultDecoder;
#[cfg(feature = "serde")]
pub use serialize::{SerializeWithSource, WithSource};
#[cfg(feature = "execution")]
pub use surface::{Matches, Parse, matches, parse};

/// Tree-sitter types used by the execution engine.
#[cfg(feature = "execution")]
pub use tree_sitter;

/// The node handle generated query outputs are built from (plus the parse
/// tree it borrows from). Re-exported so generated code and user code can
/// name them without depending on tree-sitter directly — which also
/// guarantees they see the same tree-sitter version the engine was built
/// against.
#[cfg(feature = "execution")]
pub use tree_sitter::{Node, Tree};

#[cfg(feature = "execution")]
const _: () = assert!(
    GENERATED_NODE_VALUE_BYTES >= std::mem::size_of::<Node<'static>>() as u64,
    "generated decoder-frame estimate must cover tree_sitter::Node"
);

/// Generated `SerializeWithSource` impls spell serde paths through this
/// re-export, so user crates don't need their own serde dependency.
#[cfg(feature = "serde")]
pub use serde;
