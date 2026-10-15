pub mod colors;
pub mod grammar;
mod interner;
#[cfg(feature = "vm")]
mod tree_dump;
pub mod utils;

pub use colors::Colors;
pub use interner::{Interner, Symbol};
#[cfg(feature = "vm")]
pub use tree_dump::{DumpChunk, DumpChunkKind, DumpNode, TreeDump, dump_tree, dump_tree_text};

pub use plotnik_runtime::{NodeFieldId, NodeKindId, ZeroIdError};

pub(crate) use plotnik_runtime::{NodeClass, SkipClass};

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
