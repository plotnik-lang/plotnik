#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Instruction-encoding emission phase: resolve each instruction into its
//! transition bytes.

mod instructions;

pub use instructions::emit_instructions;

use plotnik_compiler_core::ir::{CompileResult, LayoutMap};
use plotnik_compiler_core::{EmitError, RegexTableBuilder, StringTableBuilder, TypeTableBuilder};

/// Encode each instruction into transition bytes. Fans in layout, types,
/// strings, and regexes; mints nothing.
pub fn encode(
    ir: &CompileResult,
    layout: &LayoutMap,
    types: &TypeTableBuilder,
    strings: &StringTableBuilder,
    regexes: &RegexTableBuilder,
) -> Result<Vec<u8>, EmitError> {
    emit_instructions(&ir.instructions, layout, types, strings, regexes)
}
