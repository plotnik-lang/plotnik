//! Instruction-layout emission phase: assign a cache-aligned bytecode-word address to
//! every label.

mod cache_aligned;

pub use cache_aligned::CacheAligned;

use crate::compiler::emit::targets::bytecode::layout_map::LayoutMap;
use crate::compiler::emit::targets::bytecode::tables::EmitError;
use crate::compiler::lower::ir::{Label, NfaGraph};

/// Assign a cache-aligned bytecode-word address to every label.
pub fn compute_layout(ir: &NfaGraph) -> Result<LayoutMap, EmitError> {
    let entry_labels: Vec<Label> = ir
        .entry_points()
        .values()
        .map(|entry| entry.target)
        .collect();
    // With no selectable definitions, no VM can enter this module. Fragment-only
    // definitions still contribute type metadata, but their instructions are
    // unreachable and would otherwise make an arbitrary label occupy address 0.
    if entry_labels.is_empty() {
        return Ok(LayoutMap::empty());
    }

    let layout = CacheAligned::layout(ir.instructions(), &entry_labels);

    // Reject layouts whose bytecode-word addresses overflow the u16 address space.
    // `total_words` is computed in u32 precisely so this guard is reachable.
    if layout.total_words() > u16::MAX as u32 {
        return Err(EmitError::TooManyInstructionWords(
            layout.total_words() as usize
        ));
    }
    Ok(layout)
}

#[cfg(test)]
mod cache_aligned_tests;
