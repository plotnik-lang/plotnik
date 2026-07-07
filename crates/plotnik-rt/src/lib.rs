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
pub use nav::Nav;
pub use node_class::{NodeClass, SkipClass};

#[cfg(feature = "tree-sitter")]
pub use checkpoint::{CallResume, Checkpoint, CheckpointStack, CheckpointState};
#[cfg(feature = "tree-sitter")]
pub use cursor::{CursorWrapper, SkipPolicy};
#[cfg(feature = "tree-sitter")]
pub use effect::{EffectLog, RuntimeEffect};
