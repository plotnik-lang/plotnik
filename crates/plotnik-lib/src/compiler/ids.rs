//! Compiler-internal identity newtypes shared across stages.
//!
//! `DefId` indexes a named definition; `TypeId` indexes the analysis-time type
//! registry. Both are assigned during analysis and read forward by lower and
//! emit, so they live at the compiler root rather than in any single stage.

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
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Interned query type identifier.
///
/// Indexes the analysis-time type registry. This is distinct from the serialized
/// bytecode `TypeId`, which is compacted during emission. Ordered so name-table
/// iteration is deterministic (registration order).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TypeId(pub u32);

impl TypeId {
    #[inline]
    pub fn is_builtin(self) -> bool {
        self.0 < u32::from(crate::bytecode::type_system::TYPE_CUSTOM_START)
    }
}
