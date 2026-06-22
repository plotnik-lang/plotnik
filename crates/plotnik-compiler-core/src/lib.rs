#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod source;

pub use plotnik_core::{Interner, NodeFieldId, NodeKind, NodeKindId, Symbol};
pub use source::{Source, SourceId, SourceKind, SourceMap};

/// A lightweight handle to a named query definition.
///
/// Assigned during dependency analysis and shared by later compiler artifacts.
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
