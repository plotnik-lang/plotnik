use std::num::NonZeroU16;

use crate::bytecode::Nav;

use super::ir::{
    CallIR, CalleeEntry, EffectIR, InstructionIR, Label, MatchIR, NodeKindConstraint, ReturnAddr,
    ReturnIR,
};

#[test]
fn match_ir_size_match8() {
    let m = MatchIR::terminal(Label(0))
        .nav(Nav::Down)
        .node_kind(NodeKindConstraint::Named(NonZeroU16::new(10)))
        .next(Label(1));

    assert_eq!(m.size(), 8);
}

#[test]
fn match_ir_size_extended() {
    let m = MatchIR::terminal(Label(0))
        .nav(Nav::Down)
        .node_kind(NodeKindConstraint::Named(NonZeroU16::new(10)))
        .pre_effect(EffectIR::start_struct())
        .post_effect(EffectIR::node())
        .next(Label(1));

    // 3 slots needed (1 pre + 1 post + 1 succ), fits in Match16 (4 slots)
    assert_eq!(m.size(), 16);
}

#[test]
fn instruction_successors() {
    let m: InstructionIR = MatchIR::terminal(Label(0))
        .successors(vec![Label(1), Label(2)])
        .into();

    assert_eq!(m.successors(), vec![Label(1), Label(2)]);

    let c: InstructionIR = CallIR::new(Label(3), ReturnAddr(Label(4)), CalleeEntry(Label(5)))
        .nav(Nav::Down)
        .into();

    assert_eq!(c.successors(), vec![Label(4)]);

    let r: InstructionIR = ReturnIR::new(Label(6)).into();

    assert!(r.successors().is_empty());
}
