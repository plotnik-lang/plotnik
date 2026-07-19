//! Binary format section primitives.

use crate::core::{NodeFieldId, NodeKindId};

use super::ids::StringId;

pub const SYMBOL_NAME_ENTRY_SIZE: usize = 4;

/// Maps a Tree-sitter node kind ID to its string name.
#[derive(Clone, Copy, Debug)]
pub struct NodeKindEntry {
    symbol: NodeKindId,
    name: StringId,
}

impl NodeKindEntry {
    pub fn new(symbol: NodeKindId, name: StringId) -> Self {
        assert!(
            symbol.is_regular(),
            "node-kind metadata contains only regular grammar symbols"
        );
        Self { symbol, name }
    }

    pub fn symbol(self) -> NodeKindId {
        self.symbol
    }

    pub fn name(self) -> StringId {
        self.name
    }

    pub fn to_bytes(self) -> [u8; SYMBOL_NAME_ENTRY_SIZE] {
        symbol_name_bytes(u16::from(self.symbol), self.name)
    }
}

/// Maps a Tree-sitter field ID to its string name.
#[derive(Clone, Copy, Debug)]
pub struct FieldEntry {
    symbol: NodeFieldId,
    name: StringId,
}

impl FieldEntry {
    pub fn new(symbol: NodeFieldId, name: StringId) -> Self {
        Self { symbol, name }
    }

    pub fn symbol(self) -> NodeFieldId {
        self.symbol
    }

    pub fn name(self) -> StringId {
        self.name
    }

    pub fn to_bytes(self) -> [u8; SYMBOL_NAME_ENTRY_SIZE] {
        symbol_name_bytes(u16::from(self.symbol), self.name)
    }
}

fn symbol_name_bytes(symbol: u16, name: StringId) -> [u8; SYMBOL_NAME_ENTRY_SIZE] {
    let mut bytes = [0; SYMBOL_NAME_ENTRY_SIZE];
    bytes[..2].copy_from_slice(&symbol.to_le_bytes());
    bytes[2..].copy_from_slice(&u16::from(name).to_le_bytes());
    bytes
}
