//! Grammar-derived id newtypes shared by the compiler and the runtime.
//!
//! The values are tree-sitter's own numeric ids: the compiler resolves names to
//! ids during grammar binding by replicating tree-sitter's symbol numbering, and the
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

/// A raw value is not a public/matchable Tree-sitter node kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidNodeKindId {
    Zero,
    ErrorRepeat,
}

impl std::fmt::Display for InvalidNodeKindId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Zero => f.write_str("node kind id must be non-zero"),
            Self::ErrorRepeat => {
                f.write_str("node kind id is Tree-sitter's internal error-repeat symbol")
            }
        }
    }
}

impl std::error::Error for InvalidNodeKindId {}

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

/// Node kind ID.
///
/// Tree-sitter uses `u16`: zero is its end symbol, `0xfffe` is its internal
/// error-repeat symbol, and `0xffff` is the public `ERROR` kind. This type
/// represents regular grammar ids (`1..=0xfffd`) and `ERROR`; error-repeat is
/// deliberately unrepresentable.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct NodeKindId(NonZeroU16);

impl TryFrom<NonZeroU16> for NodeKindId {
    type Error = InvalidNodeKindId;

    #[inline]
    fn try_from(id: NonZeroU16) -> Result<Self, Self::Error> {
        if id.get() == Self::ERROR_REPEAT_RAW {
            return Err(InvalidNodeKindId::ErrorRepeat);
        }
        Ok(Self(id))
    }
}

impl From<NodeKindId> for NonZeroU16 {
    #[inline]
    fn from(id: NodeKindId) -> Self {
        id.0
    }
}

impl From<NodeKindId> for u16 {
    #[inline]
    fn from(id: NodeKindId) -> Self {
        id.0.get()
    }
}

impl TryFrom<u16> for NodeKindId {
    type Error = InvalidNodeKindId;

    #[inline]
    fn try_from(id: u16) -> Result<Self, Self::Error> {
        let id = NonZeroU16::new(id).ok_or(InvalidNodeKindId::Zero)?;
        Self::try_from(id)
    }
}

impl std::fmt::Display for NodeKindId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.get())
    }
}

impl NodeKindId {
    const ERROR_REPEAT_RAW: u16 = u16::MAX - 1;

    /// Highest regular grammar symbol ID.
    pub const MAX_REGULAR: Self = Self(
        NonZeroU16::new(Self::ERROR_REPEAT_RAW - 1).expect("maximum regular symbol is non-zero"),
    );

    /// Number of IDs available to regular grammar symbols.
    pub const REGULAR_COUNT: usize = Self::MAX_REGULAR.0.get() as usize;

    /// Tree-sitter's builtin error symbol (`ts_builtin_sym_error`, `(TSSymbol)-1`).
    /// Every grammar shares it for `(ERROR)` nodes, and its metadata is always
    /// `named`, so a live error node satisfies `is_named() && kind_id() == ERROR`.
    pub const ERROR: Self =
        Self(NonZeroU16::new(u16::MAX).expect("Tree-sitter ERROR symbol is non-zero"));

    /// Whether this ID belongs to a grammar's ordinary symbol range.
    pub const fn is_regular(self) -> bool {
        self.0.get() <= Self::MAX_REGULAR.0.get()
    }
}

/// Field ID (tree-sitter uses NonZeroU16).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct NodeFieldId(NonZeroU16);

nonzero_u16_id!(NodeFieldId);

impl NodeFieldId {
    /// Const constructor for generated code, which bakes bound field ids as
    /// literals. A zero id fails at *build* time of the generated crate (const
    /// evaluation), never at runtime.
    pub const fn from_raw(id: u16) -> Self {
        match NonZeroU16::new(id) {
            Some(id) => Self(id),
            None => panic!("field id must be non-zero"),
        }
    }
}
