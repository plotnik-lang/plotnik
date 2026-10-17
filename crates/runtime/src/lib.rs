//! Shared runtime engine for Plotnik queries.
//!
//! The compiler and executors share instruction types, backtracking state,
//! regex automata, accounted storage, and execution limits. The default
//! `execution` feature adds Tree-sitter navigation, the output tape, result
//! publication, and JSON rendering. Compiler-only builds disable it to avoid
//! linking Tree-sitter's C runtime.

/// Current interface required by generated Rust query modules.
pub const RUNTIME_ABI: u32 = 0;

mod alloc;
mod checkpoint;
mod dfa;
mod frame;
mod ids;
mod limits;
mod nav;
mod node_class;

#[cfg(feature = "execution")]
mod cursor;
#[cfg(feature = "execution")]
mod engine;
#[cfg(feature = "execution")]
mod execution;
#[cfg(feature = "execution")]
pub mod json;
#[cfg(feature = "execution")]
mod result;
#[cfg(feature = "execution")]
mod surface;
#[cfg(feature = "execution")]
mod tape;

#[cfg(feature = "failpoints")]
pub use alloc::failpoints;
pub use alloc::{Account, Buffer, Meter, Shared, Text};
pub use checkpoint::{
    CallResume, Checkpoint, CheckpointStack, CheckpointState, EffectDepths, Resume, TapeMark,
};
pub use dfa::{RegexDfas, StaticDfa, deserialize_dfa};
pub use frame::{CallFrameError, Frame, FrameArena, PortId};
pub use ids::{InvalidNodeKindId, NodeFieldId, NodeKindId, ZeroIdError};
pub use limits::{
    GENERATED_NODE_VALUE_BYTES, GENERATED_VALUE_CAP, Limit, LimitExceeded, ResolvedRuntimeLimits,
    RuntimeLimitSpec,
};
pub use nav::{Nav, SkipPolicy};
pub use node_class::{NodeClass, SkipClass};

#[cfg(feature = "execution")]
pub use cursor::CursorWrapper;
#[cfg(feature = "execution")]
pub use engine::Engine;
#[cfg(feature = "execution")]
#[doc(hidden)]
pub use execution::{Each, Entry, Search, each_node, exec_node, find_all_nodes};
#[cfg(feature = "execution")]
pub use result::{
    Case, Document, Field, Iter, Match, Matches, Member, Normalizer, Publish, Publisher,
    ResultFamily, Rows, Schema, SchemaTables, Scope, Slice, Span, Step, Ty, Walk, WalkStack,
};
#[cfg(feature = "execution")]
pub use surface::{Execute, each, exec, find, find_all};
#[cfg(feature = "execution")]
pub use tape::{Events, JournalEvent, Tape, node_text, predicate_text, source_text};

/// Generic operations behind generated execution points.
#[cfg(feature = "execution")]
#[doc(hidden)]
pub mod run {
    pub use crate::execution::{each, exec, find_all};
}

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
    "generated size estimates must cover tree_sitter::Node"
);
