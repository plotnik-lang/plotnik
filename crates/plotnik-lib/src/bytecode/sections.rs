//! Internal bytecode section primitives.

use super::ids::StringId;

/// Maps a tree-sitter symbol id to its string name.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SymbolNameEntry {
    /// Tree-sitter symbol ID.
    pub symbol: u16,
    /// StringId for the symbol name.
    pub name: StringId,
}

impl SymbolNameEntry {
    /// Serialized size in bytes.
    pub const SIZE: usize = 4;

    pub fn new(symbol: u16, name: StringId) -> Self {
        Self { symbol, name }
    }
}

const _: () = assert!(std::mem::size_of::<SymbolNameEntry>() == SymbolNameEntry::SIZE);

/// Maps tree-sitter node kind IDs to their string names.
pub type NodeKindEntry = SymbolNameEntry;

/// Maps tree-sitter field IDs to their string names.
pub type FieldEntry = SymbolNameEntry;
