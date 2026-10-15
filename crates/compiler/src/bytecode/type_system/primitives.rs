pub const TYPE_NO_VALUE: u16 = 0;

pub const TYPE_NODE: u16 = 1;

pub const TYPE_TEXT: u16 = 2;

pub const TYPE_BOOL: u16 = 3;

pub const TYPE_CUSTOM_START: u16 = 4;

/// These indices belong to type analysis, not emitted type tables.
/// Determine an emitted type's kind from its `TypeDef`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u16)]
pub enum PrimitiveType {
    /// Successful matching that produces no value.
    NoValue = TYPE_NO_VALUE,
    /// A Tree-sitter node reference.
    Node = TYPE_NODE,
    /// Borrowed source text.
    Text = TYPE_TEXT,
    Bool = TYPE_BOOL,
}

impl PrimitiveType {
    #[inline]
    pub fn from_index(index: u16) -> Option<Self> {
        match index {
            TYPE_NO_VALUE => Some(Self::NoValue),
            TYPE_NODE => Some(Self::Node),
            TYPE_TEXT => Some(Self::Text),
            TYPE_BOOL => Some(Self::Bool),
            _ => None,
        }
    }

    #[inline]
    pub const fn index(self) -> u16 {
        self as u16
    }

    #[inline]
    pub fn is_builtin(index: u16) -> bool {
        index < TYPE_CUSTOM_START
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::NoValue => "NoValue",
            Self::Node => "Node",
            Self::Text => "Text",
            Self::Bool => "Bool",
        }
    }
}
