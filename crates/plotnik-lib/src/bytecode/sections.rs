//! Binary format section primitives.

use super::ids::StringId;

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
