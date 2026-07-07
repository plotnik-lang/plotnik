#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Core data structures for Plotnik grammar-derived node information.

use std::num::NonZeroU16;

pub mod colors;
pub mod grammar;
mod interner;
mod invariants;
mod tree_dump;
mod tree_json;
pub mod utils;

#[cfg(test)]
mod interner_tests;
#[cfg(test)]
mod utils_tests;

pub use colors::Colors;
pub use interner::{Interner, Symbol};
pub use tree_dump::{DumpChunk, DumpChunkKind, DumpNode, TreeDump, dump_tree, dump_tree_text};
pub use tree_json::tree_to_json;

/// Runtime/analyzer view of a tree node for sibling-skipping decisions.
///
/// At runtime these bits come from one tree-sitter node instance. In grammar
/// analysis they are an approximation by node kind; that boundary is explicit at
/// the call site that constructs the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NodeClass {
    pub(crate) anonymous: bool,
    pub(crate) extra: bool,
}

/// What kind of sibling may be skipped while searching for the next match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SkipClass {
    Any,
    Trivia,
    Extras,
    Exact,
}

impl SkipClass {
    pub(crate) fn admits(self, node: NodeClass) -> bool {
        match self {
            Self::Any => true,
            Self::Trivia => node.anonymous || node.extra,
            Self::Extras => node.extra,
            Self::Exact => false,
        }
    }
}

/// A raw `0` was supplied where a non-zero id is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZeroIdError;

impl std::fmt::Display for ZeroIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("id must be non-zero")
    }
}

impl std::error::Error for ZeroIdError {}

macro_rules! nonzero_u16_id {
    ($Name:ident) => {
        impl From<NonZeroU16> for $Name {
            #[inline]
            fn from(n: NonZeroU16) -> Self {
                Self(n)
            }
        }

        impl From<$Name> for NonZeroU16 {
            #[inline]
            fn from(v: $Name) -> Self {
                v.0
            }
        }

        impl From<$Name> for u16 {
            #[inline]
            fn from(v: $Name) -> Self {
                v.0.get()
            }
        }

        impl TryFrom<u16> for $Name {
            type Error = ZeroIdError;

            #[inline]
            fn try_from(n: u16) -> Result<Self, Self::Error> {
                NonZeroU16::new(n).map(Self).ok_or(ZeroIdError)
            }
        }

        impl std::fmt::Display for $Name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0.get())
            }
        }
    };
}

/// Concrete node kind identity, preserving tree-sitter's named/anonymous namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind<T> {
    Named(T),
    Anonymous(T),
}

/// Node kind ID (tree-sitter uses u16, but 0 is internal-only).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct NodeKindId(NonZeroU16);

nonzero_u16_id!(NodeKindId);

/// Field ID (tree-sitter uses NonZeroU16).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct NodeFieldId(NonZeroU16);

nonzero_u16_id!(NodeFieldId);

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
