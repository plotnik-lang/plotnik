//! Bytecode emission.
//!
//! The pipeline runs as per-phase modules under `compiler::emit`; the data they
//! share lives in `tables`. This module fixes the phase order.

mod instructions;
mod layout;
mod layout_map;
mod module;
mod regex_table;
mod string_table;
pub(in crate::compiler) mod tables;
mod type_table;

#[cfg(test)]
mod regex_table_tests;

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::emit::instructions::emit_instructions;
use crate::compiler::emit::layout::compute_layout;
use crate::compiler::emit::module::EmitPipeline;
use crate::compiler::emit::regex_table::build_regex_table;
use crate::compiler::emit::string_table::seed_string_table;
use crate::compiler::emit::tables::{ConstantPool, EmitError};
use crate::compiler::emit::type_table::build_type_table;
use crate::compiler::lower::ir::LoweredNfa;

/// Emit bytecode without the debug load self-check. Used by callers that load
/// the bytecode themselves (e.g. `check`'s dry run) and want a malformed-bytecode
/// case to surface as a diagnostic rather than the debug panic in [`emit`].
pub(in crate::compiler) fn emit_unchecked(
    input: AnalysisArtifacts<'_>,
    lowered_ir: &LoweredNfa,
) -> Result<Vec<u8>, EmitError> {
    let nfa = lowered_ir.raw();
    let strings = seed_string_table(nfa)?;
    let (types, strings) = build_type_table(&input, strings)?;
    let layout = compute_layout(nfa)?;
    let mut pipeline = EmitPipeline::new(input, nfa, strings, types, layout);

    let tables = pipeline.build_tables()?;
    let regexes = build_regex_table(nfa, pipeline.strings())?;
    let pool = ConstantPool::new(pipeline.types(), pipeline.strings(), &regexes);
    let transitions = emit_instructions(nfa.instructions(), pipeline.layout(), pool)?;

    pipeline.write_module(pool, &tables, &transitions)
}

/// Emit bytecode, asserting in debug/test builds that the loader accepts it.
///
/// In debug/test builds this proves the emitter only ever produces bytecode the
/// loader accepts: every emission is gated through the full structural
/// validator. This makes "the compiler never emits invalid bytecode" an
/// enforced invariant across the whole test suite — and the load-time
/// checks (including the effect-stack verifier) the trust gate relies on
/// double as an emit-correctness oracle. Compiled out in release, where the
/// CLI's own `Module::load(...).expect(...)` is the boundary instead.
///
/// `check` deliberately bypasses this via [`emit_unchecked`]: it loads the
/// bytecode itself and reports a rejection as a diagnostic, so it must never
/// reach this panic, in debug or release.
pub(in crate::compiler) fn emit(
    input: AnalysisArtifacts<'_>,
    lowered_ir: &LoweredNfa,
) -> Result<Vec<u8>, EmitError> {
    let output = emit_unchecked(input, lowered_ir)?;
    #[cfg(debug_assertions)]
    if let Err(err) = crate::bytecode::Module::load(&output) {
        panic!("compiler emitted bytecode rejected by Module::load: {err:?}");
    }
    Ok(output)
}
