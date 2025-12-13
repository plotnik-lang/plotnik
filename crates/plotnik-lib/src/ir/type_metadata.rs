//! Type metadata for code generation and validation.
//!
//! Type metadata is descriptive, not prescriptive—it describes what
//! transitions produce, not how they execute.

use super::Slice;
use super::ids::{StringId, TypeId};

/// First composite type ID (after primitives 0-2).
pub const TYPE_COMPOSITE_START: TypeId = 3;

/// Type definition in the compiled query.
///
/// The `members` field has dual semantics based on `kind`:
/// - Wrappers (Optional/ArrayStar/ArrayPlus): `members.start_index` is inner TypeId
/// - Composites (Record/Enum): `members` is slice into type_members segment
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TypeDef {
    pub kind: TypeKind,
    _pad: u8,
    /// Synthetic or explicit type name. `0xFFFF` for unnamed wrappers.
    pub name: StringId,
    /// See struct-level docs for dual semantics.
    pub members: Slice<TypeMember>,
    _pad2: u16,
}

// Size is 12 bytes: kind(1) + pad(1) + name(2) + members(6) + pad2(2)
// Alignment is 2 due to packed Slice<T> having align 1
const _: () = assert!(size_of::<TypeDef>() == 12);

impl TypeDef {
    /// Create a wrapper type (Optional, ArrayStar, ArrayPlus).
    pub fn wrapper(kind: TypeKind, inner: TypeId) -> Self {
        debug_assert!(matches!(
            kind,
            TypeKind::Optional | TypeKind::ArrayStar | TypeKind::ArrayPlus
        ));
        Self {
            kind,
            _pad: 0,
            name: 0xFFFF,
            members: Slice::from_inner_type(inner),
            _pad2: 0,
        }
    }

    /// Create a composite type (Record, Enum).
    pub fn composite(kind: TypeKind, name: StringId, members: Slice<TypeMember>) -> Self {
        debug_assert!(matches!(kind, TypeKind::Record | TypeKind::Enum));
        Self {
            kind,
            _pad: 0,
            name,
            members,
            _pad2: 0,
        }
    }

    /// For wrapper types, returns the inner type ID.
    pub fn inner_type(&self) -> Option<TypeId> {
        match self.kind {
            TypeKind::Optional | TypeKind::ArrayStar | TypeKind::ArrayPlus => {
                Some(self.members.start_index() as TypeId)
            }
            TypeKind::Record | TypeKind::Enum => None,
        }
    }

    /// For composite types, returns the members slice.
    pub fn members_slice(&self) -> Option<Slice<TypeMember>> {
        match self.kind {
            TypeKind::Record | TypeKind::Enum => Some(self.members),
            TypeKind::Optional | TypeKind::ArrayStar | TypeKind::ArrayPlus => None,
        }
    }

    pub fn is_wrapper(&self) -> bool {
        matches!(
            self.kind,
            TypeKind::Optional | TypeKind::ArrayStar | TypeKind::ArrayPlus
        )
    }

    pub fn is_composite(&self) -> bool {
        matches!(self.kind, TypeKind::Record | TypeKind::Enum)
    }
}

/// Discriminant for type definitions.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    /// `T?` — nullable wrapper
    Optional = 0,
    /// `T*` — zero or more elements
    ArrayStar = 1,
    /// `T+` — one or more elements (non-empty)
    ArrayPlus = 2,
    /// Struct with named fields
    Record = 3,
    /// Tagged union (discriminated)
    Enum = 4,
}

/// Member of a Record (field) or Enum (variant).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TypeMember {
    /// Field name or variant tag.
    pub name: StringId,
    /// Field type or variant payload. `TYPE_VOID` for unit variants.
    pub ty: TypeId,
}

const _: () = assert!(size_of::<TypeMember>() == 4);
const _: () = assert!(align_of::<TypeMember>() == 2);

impl TypeMember {
    pub fn new(name: StringId, ty: TypeId) -> Self {
        Self { name, ty }
    }
}
