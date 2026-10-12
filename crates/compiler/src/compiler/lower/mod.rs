use crate::compiler::lower::collapse::collapse_up;
use crate::compiler::lower::dead::remove_unreachable;
use crate::compiler::lower::dedup::dedup_states;
use crate::compiler::lower::epsilon::eliminate_epsilons;
use crate::compiler::lower::ir::{LoweredNfa, SemanticNfa};
use crate::compiler::lower::pack::pack_instructions;
use crate::compiler::lower::thompson::NfaBuilder;
use crate::compiler::lower::verify::{run_root_pruning_verified, run_verified, verify_constructed};

pub(crate) mod boundary;
pub mod collapse;
pub mod dead;
pub mod dedup;
pub(crate) mod dump;
pub mod epsilon;
mod input;
pub mod ir;
pub mod pack;
pub(crate) mod spans;
pub mod thompson;
mod verify;

#[cfg(test)]
mod spans_tests;

pub(crate) use input::LowerInput;

/// Build and optimize the NFA up to the executor fork point (see [`SemanticNfa`]).
pub(crate) fn lower_semantic(input: &LowerInput<'_>) -> SemanticNfa {
    let mut ir = NfaBuilder::build_ir(input);
    verify_constructed(&ir, input);
    run_verified("eliminate_epsilons", &mut ir, input, eliminate_epsilons);
    run_root_pruning_verified("remove_unreachable", &mut ir, input, remove_unreachable);
    run_verified("collapse_up", &mut ir, input, collapse_up);
    // Dedup is a bisimulation quotient, which the path fingerprint cannot
    // survive: merging twin states on a loop shifts the walker's cycle cut a
    // hop earlier, so recorded path sets differ even though per-path semantics
    // are identical (see the `dedup::states` module docs). It gets the
    // structural + scope-balance check instead of `run_verified`.
    dedup_states(&mut ir);
    verify_constructed(&ir, input);
    SemanticNfa::new(ir)
}

/// Pack the fork-point NFA for the wire: split instructions that exceed
/// bytecode slot limits into epsilon cascades.
pub(crate) fn pack_lowered(semantic: SemanticNfa, input: &LowerInput<'_>) -> LoweredNfa {
    let mut ir = semantic.into_raw();
    run_verified("pack_instructions", &mut ir, input, pack_instructions);
    verify_constructed(&ir, input);
    LoweredNfa::new(ir)
}
