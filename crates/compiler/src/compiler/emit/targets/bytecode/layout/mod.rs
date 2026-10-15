mod cache_aligned;

pub use cache_aligned::CacheAligned;

use crate::compiler::emit::targets::bytecode::layout_map::LayoutMap;
use crate::compiler::emit::targets::bytecode::tables::EmitError;
use crate::compiler::lower::ir::{Label, NfaGraph};

pub fn compute_layout(ir: &NfaGraph) -> Result<LayoutMap, EmitError> {
    let entry_labels: Vec<Label> = ir.executable_entries().map(|entry| entry.target).collect();
    if entry_labels.is_empty() {
        return Ok(LayoutMap::empty());
    }

    CacheAligned::layout(ir.instructions(), &entry_labels)
}

#[cfg(test)]
mod cache_aligned_tests;
