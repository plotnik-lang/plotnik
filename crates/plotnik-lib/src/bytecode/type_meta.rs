//! Type metadata definitions for the bytecode format.

use super::{StringId, TypeId};

pub use crate::bytecode::type_system::TypeKind;

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
/// - Record/Variant: `payload` = member index, `count` = member count
/// - Alias: `payload` = target TypeId, `count` = 0
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TypeDef {
    /// For wrappers/alias: inner/target TypeId.
    /// For Record/Variant: index into TypeMembers section.
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
    /// Primitive types: Void, Node, Text, Bool.
    Primitive(TypeKind),
    /// Wrapper types: Optional, ArrayZeroOrMore, ArrayOneOrMore, Alias.
    Wrapper { kind: TypeKind, inner: TypeId },
    /// A fixed set of named fields.
    Record { member_start: u16, member_count: u8 },
    /// Variant type with named cases.
    Variant { member_start: u16, member_count: u8 },
}

impl TypeDef {
    /// Serialized size in bytes.
    pub const SIZE: usize = 4;

    /// Create a builtin type (Void, Node, Text, or Bool).
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
            payload: u16::from(inner),
            count: 0,
            kind: kind as u8,
        }
    }

    /// Shared byte-writer for the two member-run kinds (Record, Variant). The
    /// `kind as u8` write here is the on-wire discriminant — `for_record` and
    /// `for_variant` are the only callers.
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

    pub fn for_record(member_start: u16, member_count: u8) -> Self {
        Self::composite(TypeKind::Record, member_start, member_count)
    }

    pub fn for_variant(member_start: u16, member_count: u8) -> Self {
        Self::composite(TypeKind::Variant, member_start, member_count)
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            payload: u16::from_le_bytes([bytes[0], bytes[1]]),
            count: bytes[2],
            kind: bytes[3],
        }
    }

    pub fn to_bytes(self) -> [u8; 4] {
        let mut bytes = [0u8; 4];
        bytes[0..2].copy_from_slice(&self.payload.to_le_bytes());
        bytes[2] = self.count;
        bytes[3] = self.kind;
        bytes
    }

    /// Member range `(start, count)` as stored, regardless of kind.
    ///
    /// Meaningful only for Record/Variant, where `start` indexes TypeMembers and
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
    /// for bytecode validation.
    pub fn try_decode(&self) -> Option<TypeDefKind> {
        let kind = TypeKind::from_u8(self.kind)?;
        Some(match kind {
            TypeKind::Void | TypeKind::Node | TypeKind::Text | TypeKind::Bool => {
                TypeDefKind::Primitive(kind)
            }
            TypeKind::Optional
            | TypeKind::ArrayZeroOrMore
            | TypeKind::ArrayOneOrMore
            | TypeKind::Alias => TypeDefKind::Wrapper {
                kind,
                inner: TypeId::from(self.payload),
            },
            TypeKind::Record => TypeDefKind::Record {
                member_start: self.payload,
                member_count: self.count,
            },
            TypeKind::Variant => TypeDefKind::Variant {
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

    pub fn to_bytes(self) -> [u8; 4] {
        let mut bytes = [0u8; 4];
        bytes[0..2].copy_from_slice(&u16::from(self.name_id).to_le_bytes());
        bytes[2..4].copy_from_slice(&u16::from(self.type_id).to_le_bytes());
        bytes
    }
}

/// Field or case entry (4 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TypeMember {
    /// Field/case name.
    pub name_id: StringId,
    /// Type of this field/case.
    pub type_id: TypeId,
}

const _: () = assert!(std::mem::size_of::<TypeMember>() == TypeMember::SIZE);

impl TypeMember {
    /// Serialized size in bytes.
    pub const SIZE: usize = 4;

    pub fn new(name_id: StringId, type_id: TypeId) -> Self {
        Self { name_id, type_id }
    }

    pub fn to_bytes(self) -> [u8; 4] {
        let mut bytes = [0u8; 4];
        bytes[0..2].copy_from_slice(&u16::from(self.name_id).to_le_bytes());
        bytes[2..4].copy_from_slice(&u16::from(self.type_id).to_le_bytes());
        bytes
    }
}
