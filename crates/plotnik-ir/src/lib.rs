#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[path = "../../plotnik-compiler/src/bytecode/ir.rs"]
mod ir;

pub use ir::{
    CallIR, CompileResult, EffectArg, EffectIR, InstructionIR, Label, LayoutMap, MatchIR,
    MemberRef, NodeKindConstraint, PredicateIR, PredicateValueIR, ReturnIR, TrampolineIR,
};
