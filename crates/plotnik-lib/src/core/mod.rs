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

/// A raw `0` was supplied where a non-zero id is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZeroIdError;

impl std::fmt::Display for ZeroIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("id must be non-zero")
    }
}

impl std::error::Error for ZeroIdError {}

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

impl From<NonZeroU16> for NodeKindId {
    #[inline]
    fn from(n: NonZeroU16) -> Self {
        Self(n)
    }
}
impl From<NodeKindId> for NonZeroU16 {
    #[inline]
    fn from(v: NodeKindId) -> Self {
        v.0
    }
}
impl From<NodeKindId> for u16 {
    #[inline]
    fn from(v: NodeKindId) -> Self {
        v.0.get()
    }
}
impl TryFrom<u16> for NodeKindId {
    type Error = ZeroIdError;
    #[inline]
    fn try_from(n: u16) -> Result<Self, Self::Error> {
        NonZeroU16::new(n).map(Self).ok_or(ZeroIdError)
    }
}
impl std::fmt::Display for NodeKindId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.get())
    }
}

/// Field ID (tree-sitter uses NonZeroU16).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct NodeFieldId(NonZeroU16);

impl From<NonZeroU16> for NodeFieldId {
    #[inline]
    fn from(n: NonZeroU16) -> Self {
        Self(n)
    }
}
impl From<NodeFieldId> for NonZeroU16 {
    #[inline]
    fn from(v: NodeFieldId) -> Self {
        v.0
    }
}
impl From<NodeFieldId> for u16 {
    #[inline]
    fn from(v: NodeFieldId) -> Self {
        v.0.get()
    }
}
impl TryFrom<u16> for NodeFieldId {
    type Error = ZeroIdError;
    #[inline]
    fn try_from(n: u16) -> Result<Self, Self::Error> {
        NonZeroU16::new(n).map(Self).ok_or(ZeroIdError)
    }
}
impl std::fmt::Display for NodeFieldId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.get())
    }
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
