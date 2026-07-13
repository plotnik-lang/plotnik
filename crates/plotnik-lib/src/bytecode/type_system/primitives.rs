//! Primitive (builtin) type definitions.
//!
//! These are the fundamental types that exist in every query,
//! with fixed indices 0 through 3 reserved across analysis and bytecode.

/// Index for the Void type (produces nothing).
pub const TYPE_VOID: u16 = 0;

/// Index for the Node type (tree-sitter AST node reference).
pub const TYPE_NODE: u16 = 1;

/// Index for borrowed source text.
pub const TYPE_STR: u16 = 2;

/// Index for boolean values.
pub const TYPE_BOOL: u16 = 3;

/// First index available for user-defined/composite types.
pub const TYPE_CUSTOM_START: u16 = 4;

/// Builtin scalar types; no additional metadata in the type table.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u16)]
pub enum PrimitiveType {
    /// Produces nothing, transparent to parent scope.
    Void = TYPE_VOID,
    /// A tree-sitter AST node reference.
    Node = TYPE_NODE,
    /// Borrowed source text.
    Str = TYPE_STR,
    /// Boolean value.
    Bool = TYPE_BOOL,
}

impl PrimitiveType {
    /// Try to convert a type index to a primitive type.
    #[inline]
    pub fn from_index(index: u16) -> Option<Self> {
        match index {
            TYPE_VOID => Some(Self::Void),
            TYPE_NODE => Some(Self::Node),
            TYPE_STR => Some(Self::Str),
            TYPE_BOOL => Some(Self::Bool),
            _ => None,
        }
    }

    /// Get the type index for this primitive.
    #[inline]
    pub const fn index(self) -> u16 {
        self as u16
    }

    /// Check if a type index is a builtin primitive.
    #[inline]
    pub fn is_builtin(index: u16) -> bool {
        index < TYPE_CUSTOM_START
    }

    /// Get the display name for this primitive (for bytecode dumps).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Void => "Void",
            Self::Node => "Node",
            Self::Str => "Str",
            Self::Bool => "Bool",
        }
    }
}
