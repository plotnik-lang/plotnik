//! Shared runtime engine for Plotnik queries.
//!
//! Hosts the pieces of query execution that are independent of *how* the query
//! program is delivered: tree navigation ([`CursorWrapper`]), backtracking
//! state ([`CheckpointStack`], [`FrameArena`]), the capture effect log
//! ([`EffectLog`]), regex-predicate automata ([`RegexDfas`]), execution limits,
//! and the type vocabulary shared with the compiler ([`Nav`], [`NodeKindId`],
//! ...). The bytecode VM in `plotnik-lib` interprets query programs on top of
//! these primitives; generated Rust matchers (the proc-macro backend) compile
//! to direct calls into the same primitives — engine semantics stay
//! single-sourced.
//!
//! The `tree-sitter` feature (default) gates exactly the modules that touch a
//! parse tree. With `default-features = false` only the tree-free vocabulary
//! remains, which is what lets `plotnik-lib`'s compiler build without linking
//! tree-sitter's C runtime.

mod dfa;
mod frame;
mod ids;
mod limits;
mod nav;
mod node_class;

#[cfg(feature = "tree-sitter")]
mod checkpoint;
#[cfg(feature = "tree-sitter")]
mod cursor;
#[cfg(feature = "tree-sitter")]
mod effect;
#[cfg(feature = "serde")]
mod serialize;

#[cfg(test)]
#[cfg(feature = "tree-sitter")]
mod checkpoint_tests;
#[cfg(test)]
mod dfa_tests;
#[cfg(test)]
mod nav_tests;

pub use dfa::{RegexDfas, deserialize_dfa};
pub use frame::{Frame, FrameArena};
pub use ids::{NodeFieldId, NodeKindId, ZeroIdError};
pub use limits::{Limit, ResolvedRuntimeLimits, RuntimeLimitSpec};
pub use nav::{Nav, SkipPolicy};
pub use node_class::{NodeClass, SkipClass};

#[cfg(feature = "tree-sitter")]
pub use checkpoint::{CallResume, Checkpoint, CheckpointStack, CheckpointState, Resume};
#[cfg(feature = "tree-sitter")]
pub use cursor::CursorWrapper;
#[cfg(feature = "tree-sitter")]
pub use effect::{EffectLog, RuntimeEffect};
#[cfg(feature = "serde")]
pub use serialize::{SerializeWithSource, WithSource};

/// The node handle generated query outputs are built from. Re-exported so
/// generated code and user code can name it without depending on tree-sitter
/// directly — which also guarantees they see the same tree-sitter version the
/// engine was built against.
#[cfg(feature = "tree-sitter")]
pub use tree_sitter::Node;

/// Generated `SerializeWithSource` impls spell serde paths through this
/// re-export, so user crates don't need their own serde dependency.
#[cfg(feature = "serde")]
pub use serde;
