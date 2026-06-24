use std::num::NonZeroU16;

use crate::bytecode::{EffectKind, Nav, PredicateOp};

use super::ir::{
    CallIR, CalleeEntry, EffectIR, InstructionIR, Label, MatchIR, NodeKindConstraint, PredicateIR,
    PredicateValueIR, ReturnAddr, ReturnIR,
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

#[test]
fn effect_ir_preserves_literal_payload() {
    let simple = EffectIR::literal(EffectKind::Node, 5);
    assert_eq!(simple.kind(), EffectKind::Node);
}

#[test]
fn predicate_ir_stores_text_until_emit() {
    let string = PredicateIR::string(PredicateOp::Eq, "hello");
    assert_eq!(string.value.text(), "hello");
    assert!(!string.value.is_regex());

    let regex = PredicateIR::regex(PredicateOp::RegexMatch, "h.*o");
    assert_eq!(regex.value, PredicateValueIR::Regex("h.*o".into()));
    assert!(regex.value.is_regex());
}
