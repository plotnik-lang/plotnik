use crate::compiler::lower::collapse::collapse_up;
use crate::compiler::lower::dead::remove_unreachable;
use crate::compiler::lower::epsilon::eliminate_epsilons;
use crate::compiler::lower::ir::LoweredNfa;
use crate::compiler::lower::pack::pack_instructions;
use crate::compiler::lower::thompson::NfaBuilder;
use crate::compiler::lower::verify::{run_verified, verify_constructed};

pub mod collapse;
pub mod dead;
pub mod epsilon;
mod input;
pub mod ir;
pub mod pack;
pub mod thompson;
mod verify;

#[cfg(test)]
mod ir_tests;

pub(crate) use input::LowerInput;

pub(crate) fn lower_to_nfa(input: LowerInput<'_>) -> LoweredNfa {
    let mut ir = NfaBuilder::build_ir(&input);
    verify_constructed(&ir, &input);
    run_verified("eliminate_epsilons", &mut ir, &input, eliminate_epsilons);
    run_verified("remove_unreachable", &mut ir, &input, remove_unreachable);
    run_verified("collapse_up", &mut ir, &input, collapse_up);
    run_verified("pack_instructions", &mut ir, &input, pack_instructions);
    verify_constructed(&ir, &input);

    LoweredNfa::new(ir)
}
