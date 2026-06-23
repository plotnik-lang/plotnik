//! The emit driver: sequences the per-phase `emit-*` passes into a module.
//!
//! Each phase lives in its own module and depends only on `compiler::core`; this
//! driver is the one place that depends on them all and fixes their order. The string
//! table is the cross-phase accumulator — `intern_predicates` creates it,
//! `build_type_table` and `build_tables` extend it (so it is threaded by value
//! through them), and the rest only read it. Insertion order fixes StringIds, so
//! the order of these calls matters.

use crate::compiler::core::ir::CompileResult;
use crate::compiler::core::{EmitError, EmitInput};

use crate::compiler::emit::instructions::encode;
use crate::compiler::emit::layout::compute_layout;
use crate::compiler::emit::module::{build_tables, write_module};
use crate::compiler::emit::regex::build_regex_table;
use crate::compiler::emit::strings::intern_predicates;
use crate::compiler::emit::types::build_type_table;

/// Emit bytecode without the debug load self-check. Used by callers that load
/// the bytecode themselves (e.g. `check`'s dry run) and want a malformed-bytecode
/// case to surface as a diagnostic rather than the debug panic in [`emit`].
pub fn emit_unchecked(
    input: EmitInput<'_>,
    compile_result: &CompileResult,
) -> Result<Vec<u8>, EmitError> {
    let strings = intern_predicates(compile_result);
    let (types, strings) = build_type_table(&input, strings)?;
    let layout = compute_layout(compile_result)?;
    let (tables, strings) = build_tables(&input, compile_result, &types, &layout, strings)?;
    let regexes = build_regex_table(compile_result, &strings)?;
    let transitions = encode(compile_result, &layout, &types, &strings, &regexes)?;

    Ok(write_module(
        &strings,
        &types,
        &regexes,
        &layout,
        &tables,
        &transitions,
    ))
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
pub fn emit(input: EmitInput<'_>, compile_result: &CompileResult) -> Result<Vec<u8>, EmitError> {
    let output = emit_unchecked(input, compile_result)?;
    #[cfg(debug_assertions)]
    if let Err(err) = crate::bytecode::Module::load(&output) {
        panic!("compiler emitted bytecode rejected by Module::load: {err:?}");
    }
    Ok(output)
}
