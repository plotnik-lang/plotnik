//! Canonical type kind definitions.
//!
//! This enum represents the semantic type kinds shared across the system.
//! Different modules may have their own representations that map to/from this.

/// Semantic type kinds.
///
/// This is the canonical enumeration of composite type kinds.
/// Primitive types (Void, Node, String) are handled via reserved indices,
/// not as variants here.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum TypeKind {
    /// `T?` - optional wrapper, contains zero or one value.
    Optional = 0,
    /// `T*` - array of zero or more values.
    ArrayZeroOrMore = 1,
    /// `T+` - array of one or more values (non-empty).
    ArrayOneOrMore = 2,
    /// Record with named fields.
    Struct = 3,
    /// Discriminated union with tagged variants.
    Enum = 4,
    /// Named reference to another type (e.g., `type Foo = Bar`).
    Alias = 5,
}

impl TypeKind {
    /// Convert from raw discriminant.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Optional),
            1 => Some(Self::ArrayZeroOrMore),
            2 => Some(Self::ArrayOneOrMore),
            3 => Some(Self::Struct),
            4 => Some(Self::Enum),
            5 => Some(Self::Alias),
            _ => None,
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_u8_valid() {
        assert_eq!(TypeKind::from_u8(0), Some(TypeKind::Optional));
        assert_eq!(TypeKind::from_u8(1), Some(TypeKind::ArrayZeroOrMore));
        assert_eq!(TypeKind::from_u8(2), Some(TypeKind::ArrayOneOrMore));
        assert_eq!(TypeKind::from_u8(3), Some(TypeKind::Struct));
        assert_eq!(TypeKind::from_u8(4), Some(TypeKind::Enum));
        assert_eq!(TypeKind::from_u8(5), Some(TypeKind::Alias));
    }

    #[test]
    fn from_u8_invalid() {
        assert_eq!(TypeKind::from_u8(6), None);
        assert_eq!(TypeKind::from_u8(255), None);
    }

    #[test]
    fn is_wrapper() {
        assert!(TypeKind::Optional.is_wrapper());
        assert!(TypeKind::ArrayZeroOrMore.is_wrapper());
        assert!(TypeKind::ArrayOneOrMore.is_wrapper());
        assert!(!TypeKind::Struct.is_wrapper());
        assert!(!TypeKind::Enum.is_wrapper());
        assert!(!TypeKind::Alias.is_wrapper());
    }

    #[test]
    fn is_composite() {
        assert!(!TypeKind::Optional.is_composite());
        assert!(!TypeKind::ArrayZeroOrMore.is_composite());
        assert!(!TypeKind::ArrayOneOrMore.is_composite());
        assert!(TypeKind::Struct.is_composite());
        assert!(TypeKind::Enum.is_composite());
        assert!(!TypeKind::Alias.is_composite());
    }

    #[test]
    fn is_array() {
        assert!(!TypeKind::Optional.is_array());
        assert!(TypeKind::ArrayZeroOrMore.is_array());
        assert!(TypeKind::ArrayOneOrMore.is_array());
        assert!(!TypeKind::Struct.is_array());
        assert!(!TypeKind::Enum.is_array());
        assert!(!TypeKind::Alias.is_array());
    }

    #[test]
    fn array_is_non_empty() {
        assert!(!TypeKind::ArrayZeroOrMore.array_is_non_empty());
        assert!(TypeKind::ArrayOneOrMore.array_is_non_empty());
    }

    #[test]
    fn is_alias() {
        assert!(!TypeKind::Optional.is_alias());
        assert!(!TypeKind::ArrayZeroOrMore.is_alias());
        assert!(!TypeKind::ArrayOneOrMore.is_alias());
        assert!(!TypeKind::Struct.is_alias());
        assert!(!TypeKind::Enum.is_alias());
        assert!(TypeKind::Alias.is_alias());
    }
}
