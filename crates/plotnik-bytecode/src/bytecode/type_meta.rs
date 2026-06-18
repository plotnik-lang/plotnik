//! Type metadata definitions for bytecode format.

use super::{StringId, TypeId};

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
/// Semantics of `payload` and `count` depend on `kind`:
/// - Wrappers (Optional, ArrayStar, ArrayPlus): `payload` = inner TypeId, `count` = 0
/// - Struct/Enum: `payload` = member index, `count` = member count
/// - Alias: `payload` = target TypeId, `count` = 0
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TypeDef {
    /// For wrappers/alias: inner/target TypeId.
    /// For Struct/Enum: index into TypeMembers section.
    payload: u16,
    /// Member count (0 for wrappers/alias, field/variant count for composites).
    count: u8,
    /// TypeKind discriminant.
    kind: u8,
}

const _: () = assert!(std::mem::size_of::<TypeDef>() == TypeDef::SIZE);

/// Structured view of TypeDef data, eliminating the need for Option-returning accessors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeDefKind {
    /// Primitive types: Void, Node.
    Primitive(TypeKind),
    /// Wrapper types: Optional, ArrayZeroOrMore, ArrayOneOrMore, Alias.
    Wrapper { kind: TypeKind, inner: TypeId },
    /// A fixed set of named fields.
    Struct { member_start: u16, member_count: u8 },
    /// A tagged set of variants.
    Enum { member_start: u16, member_count: u8 },
}

impl TypeDef {
    /// Serialized size in bytes.
    pub const SIZE: usize = 4;

    /// Create a builtin type (Void, Node).
    pub fn builtin(kind: TypeKind) -> Self {
        Self {
            payload: 0,
            count: 0,
            kind: kind as u8,
        }
    }

    /// Create a placeholder slot (to be filled later).
    pub fn placeholder() -> Self {
        Self {
            payload: 0,
            count: 0,
            kind: 0,
        }
    }

    /// Create a wrapper type (Optional, ArrayStar, ArrayPlus).
    pub fn wrapper(kind: TypeKind, inner: TypeId) -> Self {
        Self {
            payload: inner.0,
            count: 0,
            kind: kind as u8,
        }
    }

    /// Shared byte-writer for the two member-run kinds (Struct, Enum). The
    /// `kind as u8` write here is the on-wire discriminant — `for_struct` and
    /// `for_enum` are the only callers.
    fn composite(kind: TypeKind, member_start: u16, member_count: u8) -> Self {
        Self {
            payload: member_start,
            count: member_count,
            kind: kind as u8,
        }
    }

    pub fn optional(inner: TypeId) -> Self {
        Self::wrapper(TypeKind::Optional, inner)
    }

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

    pub fn for_struct(member_start: u16, member_count: u8) -> Self {
        Self::composite(TypeKind::Struct, member_start, member_count)
    }

    pub fn for_enum(member_start: u16, member_count: u8) -> Self {
        Self::composite(TypeKind::Enum, member_start, member_count)
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            payload: u16::from_le_bytes([bytes[0], bytes[1]]),
            count: bytes[2],
            kind: bytes[3],
        }
    }

    pub fn to_bytes(&self) -> [u8; 4] {
        let mut bytes = [0u8; 4];
        bytes[0..2].copy_from_slice(&self.payload.to_le_bytes());
        bytes[2] = self.count;
        bytes[3] = self.kind;
        bytes
    }

    /// Raw kind discriminant byte, without interpreting it.
    ///
    /// Use this for validation: unlike [`decode`](Self::decode) it never
    /// panics on an unknown kind.
    pub fn kind_byte(&self) -> u8 {
        self.kind
    }

    /// Member range `(start, count)` as stored, regardless of kind.
    ///
    /// Meaningful only for Struct/Enum, where `start` indexes TypeMembers and
    /// `count` is the field/variant count.
    pub fn member_range(&self) -> (u16, u8) {
        (self.payload, self.count)
    }

    /// Decode this type definition into a structured enum.
    ///
    /// # Panics
    /// Panics if the kind byte is invalid (corrupted bytecode). Trusted side only;
    /// at the load boundary use [`try_decode`](Self::try_decode).
    pub fn decode(&self) -> TypeDefKind {
        self.try_decode()
            .unwrap_or_else(|| panic!("invalid TypeKind byte: {}", self.kind))
    }

    /// Decode, returning `None` on an unknown kind byte instead of panicking —
    /// for load-time validation of untrusted bytecode.
    pub fn try_decode(&self) -> Option<TypeDefKind> {
        let kind = TypeKind::from_u8(self.kind)?;
        Some(match kind {
            TypeKind::Void | TypeKind::Node => TypeDefKind::Primitive(kind),
            TypeKind::Optional
            | TypeKind::ArrayZeroOrMore
            | TypeKind::ArrayOneOrMore
            | TypeKind::Alias => TypeDefKind::Wrapper {
                kind,
                inner: TypeId(self.payload),
            },
            TypeKind::Struct => TypeDefKind::Struct {
                member_start: self.payload,
                member_count: self.count,
            },
            TypeKind::Enum => TypeDefKind::Enum {
                member_start: self.payload,
                member_count: self.count,
            },
        })
    }
}

/// Maps a name to a type (4 bytes).
///
/// Only named types (definitions, aliases) have entries here.
/// Entries are sorted lexicographically by name for binary search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TypeNameEntry {
    /// StringId of the type name.
    pub name_id: StringId,
    /// TypeId this name refers to.
    pub type_id: TypeId,
}

const _: () = assert!(std::mem::size_of::<TypeNameEntry>() == TypeNameEntry::SIZE);

impl TypeNameEntry {
    /// Serialized size in bytes.
    pub const SIZE: usize = 4;

    pub fn new(name_id: StringId, type_id: TypeId) -> Self {
        Self { name_id, type_id }
    }

    pub fn to_bytes(&self) -> [u8; 4] {
        let mut bytes = [0u8; 4];
        bytes[0..2].copy_from_slice(&self.name_id.as_u16().to_le_bytes());
        bytes[2..4].copy_from_slice(&self.type_id.0.to_le_bytes());
        bytes
    }
}

/// Field or variant entry (4 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TypeMember {
    /// Field/variant name.
    pub name_id: StringId,
    /// Type of this field/variant.
    pub type_id: TypeId,
}

const _: () = assert!(std::mem::size_of::<TypeMember>() == TypeMember::SIZE);

impl TypeMember {
    /// Serialized size in bytes.
    pub const SIZE: usize = 4;

    pub fn new(name_id: StringId, type_id: TypeId) -> Self {
        Self { name_id, type_id }
    }

    pub fn to_bytes(&self) -> [u8; 4] {
        let mut bytes = [0u8; 4];
        bytes[0..2].copy_from_slice(&self.name_id.as_u16().to_le_bytes());
        bytes[2..4].copy_from_slice(&self.type_id.0.to_le_bytes());
        bytes
    }
}
