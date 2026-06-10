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

/// Cardinality info for a field or children slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cardinality {
    pub multiple: bool,
    pub required: bool,
}
