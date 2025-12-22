pub use plotnik_core::{Interner, Symbol};

/// A lightweight handle to a named definition.
///
/// Assigned during dependency analysis.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn def_id_roundtrip() {
        let id = DefId::from_raw(42);
        assert_eq!(id.as_u32(), 42);
        assert_eq!(id.index(), 42);
    }

    #[test]
    fn def_id_equality() {
        let a = DefId::from_raw(1);
        let b = DefId::from_raw(1);
        let c = DefId::from_raw(2);

        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
