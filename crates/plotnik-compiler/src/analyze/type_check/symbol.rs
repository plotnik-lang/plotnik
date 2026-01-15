pub use plotnik_core::{Interner, Symbol};

/// A lightweight handle to a named definition.
///
/// Assigned during dependency analysis. Used as a key for looking up
/// definition-level metadata (types, recursion status) in TypeContext.
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
