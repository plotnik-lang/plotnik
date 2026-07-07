//! Grammar-derived id newtypes shared by the compiler and the runtime.
//!
//! The values are tree-sitter's own numeric ids: the compiler resolves names to
//! ids at link time by replicating tree-sitter's symbol numbering, and the
//! runtime compares them against live `Node`s without further translation.

use std::num::NonZeroU16;

/// A raw `0` was supplied where a non-zero id is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZeroIdError;

impl std::fmt::Display for ZeroIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("id must be non-zero")
    }
}

impl std::error::Error for ZeroIdError {}

macro_rules! nonzero_u16_id {
    ($Name:ident) => {
        impl From<NonZeroU16> for $Name {
            #[inline]
            fn from(n: NonZeroU16) -> Self {
                Self(n)
            }
        }

        impl From<$Name> for NonZeroU16 {
            #[inline]
            fn from(v: $Name) -> Self {
                v.0
            }
        }

        impl From<$Name> for u16 {
            #[inline]
            fn from(v: $Name) -> Self {
                v.0.get()
            }
        }

        impl TryFrom<u16> for $Name {
            type Error = ZeroIdError;

            #[inline]
            fn try_from(n: u16) -> Result<Self, Self::Error> {
                NonZeroU16::new(n).map(Self).ok_or(ZeroIdError)
            }
        }

        impl std::fmt::Display for $Name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0.get())
            }
        }
    };
}

/// Node kind ID (tree-sitter uses u16, but 0 is internal-only).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct NodeKindId(NonZeroU16);

nonzero_u16_id!(NodeKindId);

impl NodeKindId {
    /// Tree-sitter's builtin error symbol (`ts_builtin_sym_error`, `(TSSymbol)-1`).
    /// Every grammar shares it for `(ERROR)` nodes, and its metadata is always
    /// `named`, so a live error node satisfies `is_named() && kind_id() == ERROR`.
    pub const ERROR: Self = Self(NonZeroU16::new(0xFFFF).expect("0xFFFF is non-zero lol"));
}

/// Field ID (tree-sitter uses NonZeroU16).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct NodeFieldId(NonZeroU16);

nonzero_u16_id!(NodeFieldId);
