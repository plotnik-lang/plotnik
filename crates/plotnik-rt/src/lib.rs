//! Shared runtime engine for Plotnik queries.
//!
//! Hosts tree navigation ([`CursorWrapper`]), the match journal
//! ([`MatchJournal`]), typed result decoding, and the execution engine. The
//! backend-independent state and instruction vocabulary come from
//! `plotnik-rt-core` and are re-exported here as one public runtime API.

#[cfg(feature = "debug")]
pub mod debug;

mod cursor;
mod engine;
mod journal;
mod result_decoder;
#[cfg(feature = "serde")]
mod serialize;
mod surface;

pub use plotnik_rt_core::*;

pub use cursor::CursorWrapper;
pub use engine::Engine;
pub use journal::{JournalEvent, MatchJournal, OutputEvents, node_text, source_text};
pub use result_decoder::ResultDecoder;
#[cfg(feature = "serde")]
pub use serialize::{SerializeWithSource, WithSource};
pub use surface::{Matches, Parse, matches, parse};

/// The selected native Tree-sitter binding.
pub use tree_sitter;

/// The node handle generated query outputs are built from (plus the parse
/// tree it borrows from). Re-exported so generated code and user code can
/// name them without depending on tree-sitter directly — which also
/// guarantees they see the same tree-sitter version the engine was built
/// against.
pub use tree_sitter::{Node, Tree};

const _: () = assert!(
    GENERATED_NODE_VALUE_BYTES >= std::mem::size_of::<Node<'static>>() as u64,
    "generated decoder-frame estimate must cover the selected runtime Node"
);

/// Generated `SerializeWithSource` impls spell serde paths through this
/// re-export, so user crates don't need their own serde dependency.
#[cfg(feature = "serde")]
pub use serde;
