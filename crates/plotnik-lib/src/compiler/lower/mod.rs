use crate::compiler::lower::dead::remove_unreachable;
use crate::compiler::lower::epsilon::eliminate_epsilons;
use crate::compiler::lower::ir::LoweredIr;
use crate::compiler::lower::nav::collapse_up;
use crate::compiler::lower::pack::lower;
use crate::compiler::lower::thompson::Compiler;
use crate::compiler::lower::verify::{run_verified, verify_constructed};

mod input;
pub mod dead;
pub mod epsilon;
pub mod ir;
pub mod nav;
pub mod pack;
pub mod thompson;
mod verify;

#[cfg(test)]
mod ir_tests;

pub(crate) use input::LowerInput;

pub(crate) fn lower_to_ir(input: LowerInput<'_>) -> LoweredIr {
    let mut ir = Compiler::build_ir(&input);
    verify_constructed(&ir, &input);
    run_verified("eliminate_epsilons", &mut ir, &input, eliminate_epsilons);
    run_verified("remove_unreachable", &mut ir, &input, remove_unreachable);
    run_verified("collapse_up", &mut ir, &input, collapse_up);
    run_verified("lower", &mut ir, &input, lower);
    verify_constructed(&ir, &input);

    LoweredIr::new(ir)
}
