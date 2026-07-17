//! Core data structures for Plotnik grammar-derived node information.

pub mod colors;
pub mod grammar;
mod interner;
mod invariants;
#[cfg(feature = "vm")]
mod tree_dump;
#[cfg(feature = "vm")]
mod tree_json;
pub mod utils;

pub use colors::Colors;
pub use interner::{Interner, Symbol};
#[cfg(feature = "vm")]
pub use tree_dump::{DumpChunk, DumpChunkKind, DumpNode, TreeDump, dump_tree, dump_tree_text};
#[cfg(feature = "vm")]
pub use tree_json::tree_to_json;

// Shared with the runtime crate: the compiler resolves names to these ids and
// classes; the engine (VM or generated code) consumes them against live trees.
pub use plotnik_rt::{NodeFieldId, NodeKindId, ZeroIdError};

pub(crate) use plotnik_rt::{NodeClass, SkipClass};

/// Concrete node kind identity, preserving tree-sitter's named/anonymous namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind<T> {
    Named(T),
    Anonymous(T),
}

/// Cardinality of a field or children slot: how many children may occupy it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    /// Exactly one (`!multiple && required`).
    ExactlyOne,
    /// Zero or one (`!multiple && !required`).
    Optional,
    /// One or more (`multiple && required`).
    OneOrMore,
    /// Zero or more (`multiple && !required`).
    ZeroOrMore,
}

impl Cardinality {
    pub fn from_flags(multiple: bool, required: bool) -> Self {
        match (multiple, required) {
            (false, true) => Self::ExactlyOne,
            (false, false) => Self::Optional,
            (true, true) => Self::OneOrMore,
            (true, false) => Self::ZeroOrMore,
        }
    }

    pub fn is_multiple(self) -> bool {
        matches!(self, Self::OneOrMore | Self::ZeroOrMore)
    }

    pub fn is_required(self) -> bool {
        matches!(self, Self::ExactlyOne | Self::OneOrMore)
    }
}
