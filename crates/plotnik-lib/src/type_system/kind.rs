//! Canonical type kind definitions.
//!
//! This enum represents the semantic type kinds shared across the system.
//! Different modules may have their own representations that map to/from this.

/// Semantic type kinds.
///
/// This is the canonical enumeration of all type kinds, including primitives.
/// Primitive types (Void, Node, String) are stored as TypeDefs like any other type.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum TypeKind {
    /// Unit type - used for definitions with no captures.
    Void = 0,
    /// AST node reference.
    Node = 1,
    /// Text content extracted from node.
    String = 2,
    /// `T?` - optional wrapper, contains zero or one value.
    Optional = 3,
    /// `T*` - array of zero or more values.
    ArrayZeroOrMore = 4,
    /// `T+` - array of one or more values (non-empty).
    ArrayOneOrMore = 5,
    /// Record with named fields.
    Struct = 6,
    /// Discriminated union with tagged variants.
    Enum = 7,
    /// Named reference to another type (e.g., `type Foo = Bar`).
    Alias = 8,
}

impl TypeKind {
    /// Convert from raw discriminant.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Void),
            1 => Some(Self::Node),
            2 => Some(Self::String),
            3 => Some(Self::Optional),
            4 => Some(Self::ArrayZeroOrMore),
            5 => Some(Self::ArrayOneOrMore),
            6 => Some(Self::Struct),
            7 => Some(Self::Enum),
            8 => Some(Self::Alias),
            _ => None,
        }
    }

    /// Whether this is a primitive/builtin type (Void, Node, String).
    pub fn is_primitive(self) -> bool {
        matches!(self, Self::Void | Self::Node | Self::String)
    }

    /// Whether this is a wrapper type (Optional, ArrayZeroOrMore, ArrayOneOrMore).
    ///
    /// Wrapper types contain a single inner type.
    /// Composite types (Struct, Enum) have named members.
    pub fn is_wrapper(self) -> bool {
        matches!(
            self,
            Self::Optional | Self::ArrayZeroOrMore | Self::ArrayOneOrMore
        )
    }

    /// Whether this is a composite type (Struct, Enum).
    pub fn is_composite(self) -> bool {
        matches!(self, Self::Struct | Self::Enum)
    }

    /// Whether this is an array type.
    pub fn is_array(self) -> bool {
        matches!(self, Self::ArrayZeroOrMore | Self::ArrayOneOrMore)
    }

    /// For array types, whether the array is non-empty.
    pub fn array_is_non_empty(self) -> bool {
        matches!(self, Self::ArrayOneOrMore)
    }

    /// Whether this is an alias type.
    pub fn is_alias(self) -> bool {
        matches!(self, Self::Alias)
    }

    /// Get the display name for primitive types.
    pub fn primitive_name(self) -> Option<&'static str> {
        match self {
            Self::Void => Some("Void"),
            Self::Node => Some("Node"),
            Self::String => Some("String"),
            _ => None,
        }
    }
}
