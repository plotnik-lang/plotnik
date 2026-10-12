//! String-table emission phase: intern the predicate strings the instruction
//! stream references.

use crate::compiler::emit::targets::bytecode::tables::{EmitError, StringTableBuilder};
use crate::compiler::lower::ir::{InstructionIR, NfaGraph};

/// The sole creator of the string table — seeds it from the predicate strings
/// reachable in the instruction stream. Later phases extend or read this table.
pub fn seed_string_table(ir: &NfaGraph) -> Result<StringTableBuilder, EmitError> {
    let mut strings = StringTableBuilder::new();
    intern_predicate_strings(ir.instructions(), &mut strings)?;
    Ok(strings)
}

fn intern_predicate_strings(
    instructions: &[InstructionIR],
    strings: &mut StringTableBuilder,
) -> Result<(), EmitError> {
    for instr in instructions {
        if let InstructionIR::Match(m) = instr
            && let Some(pred) = &m.predicate
        {
            strings.intern_str(pred.value.text())?;
        }
    }
    Ok(())
}
