//! Canonical type kind definitions.

/// Semantic type kind.
///
/// Primitive types (Void, Node) are stored as `TypeDef`s like any other
/// type — the kind field is the only thing that distinguishes them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum TypeKind {
    /// Unit type - used for definitions with no captures.
    Void = 0,
    /// AST node reference.
    Node = 1,
    /// `T?` - optional wrapper, contains zero or one value.
    Optional = 2,
    /// `T*` - array of zero or more values.
    ArrayZeroOrMore = 3,
    /// `T+` - array of one or more values (non-empty).
    ArrayOneOrMore = 4,
    /// Record with named fields.
    Struct = 5,
    /// Discriminated union with named variants.
    Enum = 6,
    /// Named reference to another type (e.g., `type Foo = Bar`).
    Alias = 7,
}

impl TypeKind {
    /// Convert from raw discriminant.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Void),
            1 => Some(Self::Node),
            2 => Some(Self::Optional),
            3 => Some(Self::ArrayZeroOrMore),
            4 => Some(Self::ArrayOneOrMore),
            5 => Some(Self::Struct),
            6 => Some(Self::Enum),
            7 => Some(Self::Alias),
            _ => None,
        }
    }

    /// Whether this is a primitive/builtin type (Void, Node).
    pub fn is_primitive(self) -> bool {
        matches!(self, Self::Void | Self::Node)
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

    pub fn is_array(self) -> bool {
        matches!(self, Self::ArrayZeroOrMore | Self::ArrayOneOrMore)
    }

    /// For array types, whether the array is non-empty.
    pub fn is_non_empty_array(self) -> bool {
        matches!(self, Self::ArrayOneOrMore)
    }

    pub fn is_alias(self) -> bool {
        matches!(self, Self::Alias)
    }

    pub fn primitive_name(self) -> Option<&'static str> {
        match self {
            Self::Void => Some("Void"),
            Self::Node => Some("Node"),
            _ => None,
        }
    }
}
