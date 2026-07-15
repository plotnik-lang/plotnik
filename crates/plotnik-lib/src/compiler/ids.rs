//! Compiler-internal identity newtypes shared across stages.
//!
//! `DefId` indexes a named definition, `TypeId` indexes the analysis-time type
//! registry, `TypeDeclId` indexes a named type declaration, and result IDs index
//! the target-neutral projection shared by lower and emit. They are assigned
//! before their consumers run, so they live at the compiler root rather than in
//! any single stage.

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

/// A named type declaration whose body is a structural [`TypeId`].
///
/// Kept distinct from `DefId`: definitions describe matching, while type
/// declarations describe result names. A value-producing definition owns one
/// type declaration, and an explicit capture type may introduce another.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TypeDeclId(u32);

impl TypeDeclId {
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

/// Stable identity of a type in the target-neutral result projection.
///
/// This is deliberately wider than the bytecode type ID. Source targets can
/// describe a result graph that a compact bytecode table rejects at its own
/// boundary.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ResultTypeId(u32);

impl ResultTypeId {
    #[inline]
    pub fn from_raw(index: u32) -> Self {
        Self(index)
    }

    #[inline]
    pub fn raw(self) -> u32 {
        self.0
    }

    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Stable identity of a record field or variant case in one compiled query.
///
/// The ID indexes the result model's dense member table. It is target-neutral:
/// bytecode, generated matchers, inspection spans, and mapped source output all
/// observe the same member identity.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ResultMemberId(u16);

impl ResultMemberId {
    #[inline]
    pub fn from_raw(index: u16) -> Self {
        Self(index)
    }

    #[inline]
    pub fn raw(self) -> u16 {
        self.0
    }

    #[inline]
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}
