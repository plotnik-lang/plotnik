use crate::compiler::lower::collapse::collapse_up;
use crate::compiler::lower::dead::remove_unreachable;
use crate::compiler::lower::dedup::dedup_states;
use crate::compiler::lower::epsilon::eliminate_epsilons;
use crate::compiler::lower::ir::{LoweredNfa, SemanticNfa};
use crate::compiler::lower::pack::pack_instructions;
use crate::compiler::lower::switch::form_switches;
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
mod switch;
pub mod thompson;
mod verify;

#[cfg(test)]
mod spans_tests;

pub(crate) use input::LowerInput;

pub(crate) fn lower_semantic(input: &LowerInput<'_>) -> SemanticNfa {
    let mut ir = NfaBuilder::build_ir(input);
    verify_constructed(&ir);
    run_verified("form_switches", &mut ir, input, form_switches);
    run_verified("eliminate_epsilons", &mut ir, input, eliminate_epsilons);
    run_root_pruning_verified("remove_unreachable", &mut ir, input, remove_unreachable);
    run_verified("collapse_up", &mut ir, input, collapse_up);
    dedup_states(&mut ir);
    verify_constructed(&ir);
    SemanticNfa::new(ir)
}

pub(crate) fn pack_lowered(semantic: SemanticNfa, input: &LowerInput<'_>) -> LoweredNfa {
    let mut ir = semantic.into_raw();
    run_verified("pack_instructions", &mut ir, input, pack_instructions);
    verify_constructed(&ir);
    LoweredNfa::new(ir)
}
