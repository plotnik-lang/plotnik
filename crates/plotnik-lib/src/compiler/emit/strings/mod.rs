#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! String-table emission phase: intern the predicate strings the instruction
//! stream references.

use crate::compiler::core::StringTableBuilder;
use crate::compiler::lower::ir::{CompileResult, InstructionIR};

/// The sole creator of the string table — seeds it from the predicate strings
/// reachable in the instruction stream. Later phases extend or read this table.
pub fn intern_predicates(ir: &CompileResult) -> StringTableBuilder {
    let mut strings = StringTableBuilder::new();
    intern_predicate_strings(&ir.instructions, &mut strings);
    strings
}

fn intern_predicate_strings(instructions: &[InstructionIR], strings: &mut StringTableBuilder) {
    for instr in instructions {
        if let InstructionIR::Match(m) = instr
            && let Some(pred) = &m.predicate
        {
            strings.get_or_intern_str(pred.value.text());
        }
    }
}
