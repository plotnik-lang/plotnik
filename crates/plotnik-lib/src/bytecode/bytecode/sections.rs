//! Binary format section primitives.

use super::ids::StringId;

/// Range into an array: [ptr..ptr+len).
///
/// Dual-use depending on context:
/// - For `TypeDef` wrappers (Optional, Array*): `ptr` is inner `TypeId`, `len` is 0.
/// - For `TypeDef` composites (Struct, Enum): `ptr` is index into TypeMember array, `len` is count.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct Slice {
    pub ptr: u16,
    pub len: u16,
}

impl Slice {
    #[inline]
    pub fn range(self) -> std::ops::Range<usize> {
        let start = self.ptr as usize;
        start..start + self.len as usize
    }
}

/// Maps tree-sitter NodeTypeId to its string name.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct NodeKindEntry {
    /// Tree-sitter node kind ID
    pub symbol: u16,
    /// StringId for the node kind name
    pub name: StringId,
}

impl NodeKindEntry {
    /// Serialized size in bytes.
    pub const SIZE: usize = 4;

    pub fn new(symbol: u16, name: StringId) -> Self {
        Self { symbol, name }
    }
}

const _: () = assert!(std::mem::size_of::<NodeKindEntry>() == NodeKindEntry::SIZE);

/// Maps tree-sitter NodeFieldId to its string name.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct FieldEntry {
    /// Tree-sitter field ID
    pub symbol: u16,
    /// StringId for the field name
    pub name: StringId,
}

impl FieldEntry {
    /// Serialized size in bytes.
    pub const SIZE: usize = 4;

    pub fn new(symbol: u16, name: StringId) -> Self {
        Self { symbol, name }
    }
}

const _: () = assert!(std::mem::size_of::<FieldEntry>() == FieldEntry::SIZE);
