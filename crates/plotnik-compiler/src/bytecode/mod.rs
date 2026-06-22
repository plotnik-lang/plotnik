//! Compile-time IR for bytecode emission.

pub(crate) use plotnik_compiler_core::ir::EffectArg;
pub use plotnik_compiler_core::ir::{
    CallIR, CompileResult, EffectIR, InstructionIR, Label, LayoutMap, MatchIR, MemberRef,
    NodeKindConstraint, PredicateIR, PredicateValueIR, ReturnIR, TrampolineIR,
};
