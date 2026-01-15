//! Primitive (builtin) type definitions.
//!
//! These are the fundamental types that exist in every query,
//! with fixed indices 0, 1, 2 reserved across both analysis and bytecode.

/// Index for the Void type (produces nothing).
pub const TYPE_VOID: u16 = 0;

/// Index for the Node type (tree-sitter AST node reference).
pub const TYPE_NODE: u16 = 1;

/// Index for the String type (extracted source text).
pub const TYPE_STRING: u16 = 2;

/// First index available for user-defined/composite types.
pub const TYPE_CUSTOM_START: u16 = 3;

/// Primitive type enumeration.
///
/// These are the builtin scalar types that don't require
/// additional metadata in the type table.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u16)]
pub enum PrimitiveType {
    /// Produces nothing, transparent to parent scope.
    Void = TYPE_VOID,
    /// A tree-sitter AST node reference.
    Node = TYPE_NODE,
    /// Extracted text from a node.
    String = TYPE_STRING,
}

impl PrimitiveType {
    /// Try to convert a type index to a primitive type.
    #[inline]
    pub fn from_index(index: u16) -> Option<Self> {
        match index {
            TYPE_VOID => Some(Self::Void),
            TYPE_NODE => Some(Self::Node),
            TYPE_STRING => Some(Self::String),
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
        index <= TYPE_STRING
    }

    /// Get the display name for this primitive (for bytecode dumps).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Void => "Void",
            Self::Node => "Node",
            Self::String => "String",
        }
    }
}
