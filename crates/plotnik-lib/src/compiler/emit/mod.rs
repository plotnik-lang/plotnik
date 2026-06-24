//! Bytecode emission.
//!
//! The pipeline runs as per-phase modules under `compiler::emit`; the data they
//! share lives in `tables`. This module fixes the phase order.

mod instructions;
mod layout;
mod module;
mod regex;
mod strings;
pub(in crate::compiler) mod tables;
mod types;

use crate::compiler::emit::instructions::encode;
use crate::compiler::emit::layout::compute_layout;
use crate::compiler::emit::module::{build_tables, write_module};
use crate::compiler::emit::regex::build_regex_table;
use crate::compiler::emit::strings::intern_predicates;
use crate::compiler::emit::tables::{ConstantPool, EmitError, EmitInput};
use crate::compiler::emit::types::build_type_table;
use crate::compiler::lower::ir::LoweredIr;

/// Emit bytecode without the debug load self-check. Used by callers that load
/// the bytecode themselves (e.g. `check`'s dry run) and want a malformed-bytecode
/// case to surface as a diagnostic rather than the debug panic in [`emit`].
pub(in crate::compiler) fn emit_unchecked(
    input: EmitInput<'_>,
    lowered_ir: &LoweredIr,
) -> Result<Vec<u8>, EmitError> {
    let compile_result = lowered_ir.raw();
    let strings = intern_predicates(compile_result);
    let (types, strings) = build_type_table(&input, strings)?;
    let layout = compute_layout(compile_result)?;
    let (tables, strings) = build_tables(&input, compile_result, &types, &layout, strings)?;
    let regexes = build_regex_table(compile_result, &strings)?;
    let pool = ConstantPool::new(&types, &strings, &regexes);
    let transitions = encode(compile_result, &layout, pool)?;

    Ok(write_module(pool, &layout, &tables, &transitions))
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
    input: EmitInput<'_>,
    lowered_ir: &LoweredIr,
) -> Result<Vec<u8>, EmitError> {
    let output = emit_unchecked(input, lowered_ir)?;
    #[cfg(debug_assertions)]
    if let Err(err) = crate::bytecode::Module::load(&output) {
        panic!("compiler emitted bytecode rejected by Module::load: {err:?}");
    }
    Ok(output)
}

#[cfg(test)]
mod capacity_tests;
#[cfg(test)]
mod layout_tests;
