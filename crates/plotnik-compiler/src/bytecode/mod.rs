//! Compile-time IR for bytecode emission.

mod ir;

#[cfg(test)]
mod ir_tests;

pub use ir::{
    CallIR, EffectIR, EmitContext, InstructionIR, Label, LayoutResult, MatchIR, MemberRef,
    NodeTypeIR, PredicateIR, PredicateValueIR, ReturnIR, TrampolineIR,
};
