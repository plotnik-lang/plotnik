//! Compile-time IR for bytecode emission.

mod ir;

#[cfg(test)]
mod ir_tests;

pub(crate) use ir::EffectArg;
pub use ir::{
    CallIR, CompileResult, EffectIR, InstructionIR, Label, LayoutMap, MatchIR, MemberRef,
    NodeKindConstraint, PredicateIR, PredicateValueIR, ReturnIR, TrampolineIR,
};
