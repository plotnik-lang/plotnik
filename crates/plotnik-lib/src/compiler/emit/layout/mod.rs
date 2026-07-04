#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Instruction-layout emission phase: assign a cache-aligned step address to
//! every label.

mod cache_aligned;

pub use cache_aligned::CacheAligned;

use crate::compiler::emit::layout_map::LayoutMap;
use crate::compiler::emit::tables::EmitError;
use crate::compiler::lower::ir::{Label, NfaGraph};

/// Assign a cache-aligned step address to every label.
pub fn compute_layout(ir: &NfaGraph) -> Result<LayoutMap, EmitError> {
    let entry_labels: Vec<Label> = ir.entrypoint_wrappers().values().copied().collect();
    let layout = CacheAligned::layout(ir.instructions(), &entry_labels);

    // Reject layouts whose step addresses overflow the u16 address space.
    // `total_steps` is computed in u32 precisely so this guard is reachable.
    if layout.total_steps() > u16::MAX as u32 {
        return Err(EmitError::TooManyTransitions(layout.total_steps() as usize));
    }
    Ok(layout)
}

#[cfg(test)]
mod cache_aligned_tests;
