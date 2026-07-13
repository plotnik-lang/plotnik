use std::collections::BTreeMap;

use crate::bytecode::CodeAddr;
use crate::compiler::lower::ir::Label;

/// Result of layout: maps labels to bytecode-word addresses.
#[derive(Clone, Debug)]
pub(in crate::compiler::emit) struct LayoutMap {
    /// Mapping from symbolic labels to concrete bytecode-word addresses.
    label_to_addr: BTreeMap<Label, CodeAddr>,
    /// Total number of bytecode words. Held as `u32` so a query whose layout overflows
    /// the `u16` address space is detectable at emit time instead of
    /// wrapping silently; `emit` rejects it before any address is used.
    total_words: u32,
}

impl LayoutMap {
    pub(in crate::compiler::emit) fn new(
        label_to_addr: BTreeMap<Label, CodeAddr>,
        total_words: u32,
    ) -> Self {
        Self {
            label_to_addr,
            total_words,
        }
    }

    pub(in crate::compiler::emit) fn empty() -> Self {
        Self {
            label_to_addr: BTreeMap::new(),
            total_words: 0,
        }
    }

    pub(in crate::compiler::emit) fn code_addrs(&self) -> &BTreeMap<Label, CodeAddr> {
        &self.label_to_addr
    }

    pub(in crate::compiler::emit) fn total_words(&self) -> u32 {
        self.total_words
    }
}
