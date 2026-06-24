#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! String-table emission phase: intern the predicate strings the instruction
//! stream references.

use crate::compiler::emit::tables::StringTableBuilder;
use crate::compiler::lower::ir::{NfaGraph, InstructionIR};

/// The sole creator of the string table — seeds it from the predicate strings
/// reachable in the instruction stream. Later phases extend or read this table.
pub fn intern_predicates(ir: &NfaGraph) -> StringTableBuilder {
    let mut strings = StringTableBuilder::new();
    intern_predicate_strings(ir.instructions(), &mut strings);
    strings
}

fn intern_predicate_strings(instructions: &[InstructionIR], strings: &mut StringTableBuilder) {
    for instr in instructions {
        if let InstructionIR::Match(m) = instr
            && let Some(pred) = &m.predicate
        {
            strings.intern_str(pred.value.text());
        }
    }
}
