#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Core data structures for Plotnik grammar-derived node information.

use std::num::NonZeroU16;

pub mod colors;
pub mod grammar;
mod interner;
mod invariants;
pub mod utils;

#[cfg(test)]
mod interner_tests;
#[cfg(test)]
mod utils_tests;

pub use colors::Colors;
pub use interner::{Interner, Symbol};

/// A lightweight handle to a named query definition.
///
/// Assigned during dependency analysis and shared by later pipeline artifacts.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DefId(u32);

impl DefId {
    #[inline]
    pub fn from_raw(index: u32) -> Self {
        Self(index)
    }

    #[inline]
    pub fn as_u32(self) -> u32 {
        self.0
    }

    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Interned query type identifier.
///
/// Indexes the analysis-time type registry. This is distinct from the serialized
/// bytecode `TypeId`, which is compacted during emission.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TypeId(pub u32);

impl TypeId {
    #[inline]
    pub fn is_builtin(self) -> bool {
        self.0 <= 1
    }
}

/// Concrete node kind identity, preserving tree-sitter's named/anonymous namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind<T> {
    Named(T),
    Anonymous(T),
}

/// Node kind ID (tree-sitter uses u16, but 0 is internal-only).
pub type NodeKindId = NonZeroU16;

/// Field ID (tree-sitter uses NonZeroU16).
pub type NodeFieldId = NonZeroU16;

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
