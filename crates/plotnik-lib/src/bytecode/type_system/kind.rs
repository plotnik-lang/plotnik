//! Canonical type kind definitions.

/// Semantic type kind.
///
/// Primitive types are stored as `TypeDef`s like any other
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
    /// Variant type with named cases.
    Variant = 6,
    /// Named reference to another type (e.g., `type Foo = Bar`).
    Alias = 7,
    /// Borrowed source text.
    Text = 8,
    /// Boolean value.
    Bool = 9,
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
            6 => Some(Self::Variant),
            7 => Some(Self::Alias),
            8 => Some(Self::Text),
            9 => Some(Self::Bool),
            _ => None,
        }
    }

    /// Whether this is a primitive/builtin type.
    pub fn is_primitive(self) -> bool {
        matches!(self, Self::Void | Self::Node | Self::Text | Self::Bool)
    }

    /// Whether this is a wrapper type (Optional, ArrayZeroOrMore, ArrayOneOrMore).
    ///
    /// Wrapper types contain a single inner type.
    /// Struct and Variant types carry named members instead.
    pub fn is_wrapper(self) -> bool {
        matches!(
            self,
            Self::Optional | Self::ArrayZeroOrMore | Self::ArrayOneOrMore
        )
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
            Self::Text => Some("Text"),
            Self::Bool => Some("Bool"),
            _ => None,
        }
    }
}
