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

/// Concrete node type identity, preserving tree-sitter's named/anonymous namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeType<T> {
    Named(T),
    Anonymous(T),
}

/// Node type ID (tree-sitter uses u16, but 0 is internal-only).
pub type NodeTypeId = NonZeroU16;

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
