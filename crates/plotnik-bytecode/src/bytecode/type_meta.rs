//! Type metadata definitions for bytecode format.

use super::{StringId, TypeId};

// Re-export the shared TypeKind
pub use crate::type_system::TypeKind;

/// Convenience aliases for bytecode-specific naming (ArrayStar/ArrayPlus).
impl TypeKind {
    /// Alias for `ArrayZeroOrMore` (T*).
    pub const ARRAY_STAR: Self = Self::ArrayZeroOrMore;
    /// Alias for `ArrayOneOrMore` (T+).
    pub const ARRAY_PLUS: Self = Self::ArrayOneOrMore;
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
    data: u16,
    /// Member count (0 for wrappers/alias, field/variant count for composites).
    count: u8,
    /// TypeKind discriminant.
    kind: u8,
}

const _: () = assert!(std::mem::size_of::<TypeDef>() == 4);

/// Structured view of TypeDef data, eliminating the need for Option-returning accessors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeData {
    /// Primitive types: Void, Node, String.
    Primitive(TypeKind),
    /// Wrapper types: Optional, ArrayZeroOrMore, ArrayOneOrMore, Alias.
    Wrapper { kind: TypeKind, inner: TypeId },
    /// Composite types: Struct, Enum.
    Composite {
        kind: TypeKind,
        member_start: u16,
        member_count: u8,
    },
}

impl TypeDef {
    /// Create a builtin type (Void, Node, String).
    pub fn builtin(kind: TypeKind) -> Self {
        Self {
            data: 0,
            count: 0,
            kind: kind as u8,
        }
    }

    /// Create a placeholder slot (to be filled later).
    pub fn placeholder() -> Self {
        Self {
            data: 0,
            count: 0,
            kind: 0,
        }
    }

    /// Create a wrapper type (Optional, ArrayStar, ArrayPlus).
    pub fn wrapper(kind: TypeKind, inner: TypeId) -> Self {
        Self {
            data: inner.0,
            count: 0,
            kind: kind as u8,
        }
    }

    /// Create a composite type (Struct, Enum).
    pub fn composite(kind: TypeKind, member_start: u16, member_count: u8) -> Self {
        Self {
            data: member_start,
            count: member_count,
            kind: kind as u8,
        }
    }

    /// Create an optional wrapper type.
    pub fn optional(inner: TypeId) -> Self {
        Self::wrapper(TypeKind::Optional, inner)
    }

    /// Create an alias type.
    pub fn alias(target: TypeId) -> Self {
        Self::wrapper(TypeKind::Alias, target)
    }

    /// Create an ArrayStar (T*) wrapper type.
    pub fn array_star(element: TypeId) -> Self {
        Self::wrapper(TypeKind::ARRAY_STAR, element)
    }

    /// Create an ArrayPlus (T+) wrapper type.
    pub fn array_plus(element: TypeId) -> Self {
        Self::wrapper(TypeKind::ARRAY_PLUS, element)
    }

    /// Create a struct type.
    pub fn struct_type(member_start: u16, member_count: u8) -> Self {
        Self::composite(TypeKind::Struct, member_start, member_count)
    }

    /// Create an enum type.
    pub fn enum_type(member_start: u16, member_count: u8) -> Self {
        Self::composite(TypeKind::Enum, member_start, member_count)
    }

    /// Decode from 4 bytes (crate-internal deserialization).
    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            data: u16::from_le_bytes([bytes[0], bytes[1]]),
            count: bytes[2],
            kind: bytes[3],
        }
    }

    /// Encode to 4 bytes.
    pub fn to_bytes(&self) -> [u8; 4] {
        let mut bytes = [0u8; 4];
        bytes[0..2].copy_from_slice(&self.data.to_le_bytes());
        bytes[2] = self.count;
        bytes[3] = self.kind;
        bytes
    }

    /// Classify this type definition into a structured enum.
    ///
    /// # Panics
    /// Panics if the kind byte is invalid (corrupted bytecode).
    pub fn classify(&self) -> TypeData {
        let kind = TypeKind::from_u8(self.kind)
            .unwrap_or_else(|| panic!("invalid TypeKind byte: {}", self.kind));
        match kind {
            TypeKind::Void | TypeKind::Node | TypeKind::String => TypeData::Primitive(kind),
            TypeKind::Optional
            | TypeKind::ArrayZeroOrMore
            | TypeKind::ArrayOneOrMore
            | TypeKind::Alias => TypeData::Wrapper {
                kind,
                inner: TypeId(self.data),
            },
            TypeKind::Struct | TypeKind::Enum => TypeData::Composite {
                kind,
                member_start: self.data,
                member_count: self.count,
            },
        }
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
    pub type_id: TypeId,
}

const _: () = assert!(std::mem::size_of::<TypeName>() == 4);

impl TypeName {
    /// Create a new type name entry.
    pub fn new(name: StringId, type_id: TypeId) -> Self {
        Self { name, type_id }
    }

    /// Encode to 4 bytes.
    pub fn to_bytes(&self) -> [u8; 4] {
        let mut bytes = [0u8; 4];
        bytes[0..2].copy_from_slice(&self.name.get().to_le_bytes());
        bytes[2..4].copy_from_slice(&self.type_id.0.to_le_bytes());
        bytes
    }
}

/// Field or variant entry (4 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TypeMember {
    /// Field/variant name.
    pub name: StringId,
    /// Type of this field/variant.
    pub type_id: TypeId,
}

const _: () = assert!(std::mem::size_of::<TypeMember>() == 4);

impl TypeMember {
    /// Create a new type member entry.
    pub fn new(name: StringId, type_id: TypeId) -> Self {
        Self { name, type_id }
    }

    /// Encode to 4 bytes.
    pub fn to_bytes(&self) -> [u8; 4] {
        let mut bytes = [0u8; 4];
        bytes[0..2].copy_from_slice(&self.name.get().to_le_bytes());
        bytes[2..4].copy_from_slice(&self.type_id.0.to_le_bytes());
        bytes
    }
}
