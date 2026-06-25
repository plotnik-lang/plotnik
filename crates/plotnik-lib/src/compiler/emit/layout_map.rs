use std::collections::BTreeMap;

use crate::bytecode::StepAddr;
use crate::compiler::lower::ir::Label;

/// Result of layout: maps labels to step addresses.
#[derive(Clone, Debug)]
pub(in crate::compiler::emit) struct LayoutMap {
    /// Mapping from symbolic labels to concrete step addresses (raw u16).
    label_to_step: BTreeMap<Label, StepAddr>,
    /// Total number of steps. Held as `u32` so a query whose layout overflows
    /// the `u16` step-address space is detectable at emit time instead of
    /// wrapping silently; `emit` rejects it before any address is used.
    total_steps: u32,
}

impl LayoutMap {
    pub(in crate::compiler::emit) fn new(
        label_to_step: BTreeMap<Label, StepAddr>,
        total_steps: u32,
    ) -> Self {
        Self {
            label_to_step,
            total_steps,
        }
    }

    pub(in crate::compiler::emit) fn empty() -> Self {
        Self {
            label_to_step: BTreeMap::new(),
            total_steps: 0,
        }
    }

    pub(in crate::compiler::emit) fn step_addrs(&self) -> &BTreeMap<Label, StepAddr> {
        &self.label_to_step
    }

    pub(in crate::compiler::emit) fn total_steps(&self) -> u32 {
        self.total_steps
    }
}
