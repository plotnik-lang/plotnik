//! Type metadata definitions for bytecode format.

use super::{QTypeId, StringId};

// Re-export the shared TypeKind
pub use crate::type_system::TypeKind;

/// Convenience aliases for bytecode-specific naming (ArrayStar/ArrayPlus).
impl TypeKind {
    /// Alias for `ArrayZeroOrMore` (T*).
    pub const ARRAY_STAR: Self = Self::ArrayZeroOrMore;
    /// Alias for `ArrayOneOrMore` (T+).
    pub const ARRAY_PLUS: Self = Self::ArrayOneOrMore;
}

/// TypeMeta section header (8 bytes).
///
/// Contains counts for the three sub-sections. Located at `type_meta_offset`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct TypeMetaHeader {
    /// Number of TypeDef entries.
    pub type_defs_count: u16,
    /// Number of TypeMember entries.
    pub type_members_count: u16,
    /// Number of TypeName entries.
    pub type_names_count: u16,
    /// Padding for alignment.
    pub(crate) _pad: u16,
}

const _: () = assert!(std::mem::size_of::<TypeMetaHeader>() == 8);

impl TypeMetaHeader {
    /// Decode from 8 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 8, "TypeMetaHeader too short");
        Self {
            type_defs_count: u16::from_le_bytes([bytes[0], bytes[1]]),
            type_members_count: u16::from_le_bytes([bytes[2], bytes[3]]),
            type_names_count: u16::from_le_bytes([bytes[4], bytes[5]]),
            _pad: u16::from_le_bytes([bytes[6], bytes[7]]),
        }
    }

    /// Encode to 8 bytes.
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        bytes[0..2].copy_from_slice(&self.type_defs_count.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.type_members_count.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.type_names_count.to_le_bytes());
        bytes[6..8].copy_from_slice(&self._pad.to_le_bytes());
        bytes
    }
}

/// Type definition entry (4 bytes).
///
/// Semantics of `data` and `count` depend on `kind`:
/// - Wrappers (Optional, ArrayStar, ArrayPlus): `data` = inner TypeId, `count` = 0
/// - Struct/Enum: `data` = member index, `count` = member count
/// - Alias: `data` = target TypeId, `count` = 0
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TypeDef {
    /// For wrappers/alias: inner/target TypeId.
    /// For Struct/Enum: index into TypeMembers section.
    pub data: u16,
    /// Member count (0 for wrappers/alias, field/variant count for composites).
    pub count: u8,
    /// TypeKind discriminant.
    pub kind: u8,
}

const _: () = assert!(std::mem::size_of::<TypeDef>() == 4);

impl TypeDef {
    /// For wrapper types, get the inner type.
    #[inline]
    pub fn inner_type(&self) -> Option<QTypeId> {
        TypeKind::from_u8(self.kind)
            .filter(|k| k.is_wrapper())
            .map(|_| QTypeId(self.data))
    }

    /// Get the TypeKind for this definition.
    #[inline]
    pub fn type_kind(&self) -> Option<TypeKind> {
        TypeKind::from_u8(self.kind)
    }

    /// Whether this is an alias type.
    #[inline]
    pub fn is_alias(&self) -> bool {
        TypeKind::from_u8(self.kind).is_some_and(|k| k.is_alias())
    }

    /// For alias types, get the target type.
    #[inline]
    pub fn alias_target(&self) -> Option<QTypeId> {
        TypeKind::from_u8(self.kind)
            .filter(|k| k.is_alias())
            .map(|_| QTypeId(self.data))
    }

    /// For Struct/Enum types, get the member index.
    #[inline]
    pub fn member_index(&self) -> Option<u16> {
        TypeKind::from_u8(self.kind)
            .filter(|k| k.is_composite())
            .map(|_| self.data)
    }

    /// For Struct/Enum types, get the member count.
    #[inline]
    pub fn member_count(&self) -> Option<u8> {
        TypeKind::from_u8(self.kind)
            .filter(|k| k.is_composite())
            .map(|_| self.count)
    }
}

/// Maps a name to a type (4 bytes).
///
/// Only named types (definitions, aliases) have entries here.
/// Entries are sorted lexicographically by name for binary search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TypeName {
    /// StringId of the type name.
    pub name: StringId,
    /// TypeId this name refers to.
    pub type_id: QTypeId,
}

const _: () = assert!(std::mem::size_of::<TypeName>() == 4);

/// Field or variant entry (4 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TypeMember {
    /// Field/variant name.
    pub name: StringId,
    /// Type of this field/variant.
    pub type_id: QTypeId,
}

const _: () = assert!(std::mem::size_of::<TypeMember>() == 4);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_meta_header_size() {
        assert_eq!(std::mem::size_of::<TypeMetaHeader>(), 8);
    }

    #[test]
    fn type_meta_header_roundtrip() {
        let header = TypeMetaHeader {
            type_defs_count: 42,
            type_members_count: 100,
            type_names_count: 5,
            ..Default::default()
        };
        let bytes = header.to_bytes();
        let decoded = TypeMetaHeader::from_bytes(&bytes);
        assert_eq!(decoded, header);
    }

    #[test]
    fn type_def_size() {
        assert_eq!(std::mem::size_of::<TypeDef>(), 4);
    }

    #[test]
    fn type_member_size() {
        assert_eq!(std::mem::size_of::<TypeMember>(), 4);
    }

    #[test]
    fn type_name_size() {
        assert_eq!(std::mem::size_of::<TypeName>(), 4);
    }

    #[test]
    fn type_kind_is_wrapper() {
        assert!(TypeKind::Optional.is_wrapper());
        assert!(TypeKind::ArrayZeroOrMore.is_wrapper());
        assert!(TypeKind::ArrayOneOrMore.is_wrapper());
        assert!(!TypeKind::Struct.is_wrapper());
        assert!(!TypeKind::Enum.is_wrapper());
    }

    #[test]
    fn type_kind_aliases() {
        // Test bytecode-friendly aliases
        assert_eq!(TypeKind::ARRAY_STAR, TypeKind::ArrayZeroOrMore);
        assert_eq!(TypeKind::ARRAY_PLUS, TypeKind::ArrayOneOrMore);
    }
}
