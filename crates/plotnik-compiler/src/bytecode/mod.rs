//! Compile-time IR for bytecode emission.

mod ir;

#[cfg(test)]
mod ir_tests;

pub use ir::{
    CallIR, EffectIR, EmitResolvers, InstructionIR, Label, LayoutMap, MatchIR, MemberRef,
    NodeKindConstraint, PredicateIR, PredicateValueIR, ReturnIR, TrampolineIR,
};
