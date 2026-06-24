#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub(crate) mod capture_mechanism;
pub(crate) mod grammar_binding;
pub(crate) mod source;
pub(crate) mod span;
pub(crate) mod type_analysis;
pub(crate) mod type_shape;

pub use crate::core::{Interner, Symbol};
pub use capture_mechanism::CaptureMechanism;
pub use grammar_binding::GrammarBinding;
pub use span::Span;
pub use type_analysis::{TypeAnalysis, TypeAnalysisBuilder};
pub use type_shape::TypeShape;

/// A lightweight handle to a named query definition.
///
/// Assigned during dependency analysis and shared by later compiler artifacts.
/// Ordered by assignment index, which is SCC processing order (leaves first):
/// iterating a `DefId`-keyed map yields definitions in emission order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
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
