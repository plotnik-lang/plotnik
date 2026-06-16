use std::collections::BTreeMap;
use std::num::NonZeroU16;

use plotnik_bytecode::{EffectOpcode, Nav};

use super::ir::{
    CallIR, EffectIR, EmitContext, InstructionIR, Label, MatchIR, MemberRef, NodeTypeIR, ReturnIR,
};
use crate::analyze::type_check::TypeId;

#[test]
fn match_ir_size_match8() {
    let m = MatchIR::at(Label(0))
        .nav(Nav::Down)
        .node_type(NodeTypeIR::Named(NonZeroU16::new(10)))
        .next(Label(1));

    assert_eq!(m.size(), 8);
}

#[test]
fn match_ir_size_extended() {
    let m = MatchIR::at(Label(0))
        .nav(Nav::Down)
        .node_type(NodeTypeIR::Named(NonZeroU16::new(10)))
        .pre_effect(EffectIR::start_obj())
        .post_effect(EffectIR::node())
        .next(Label(1));

    // 3 slots needed (1 pre + 1 post + 1 succ), fits in Match16 (4 slots)
    assert_eq!(m.size(), 16);
}

#[test]
fn instruction_successors() {
    let m: InstructionIR = MatchIR::at(Label(0))
        .next_many(vec![Label(1), Label(2)])
        .into();

    assert_eq!(m.successors(), vec![Label(1), Label(2)]);

    let c: InstructionIR = CallIR::new(Label(3), Label(5), Label(4))
        .nav(Nav::Down)
        .into();

    assert_eq!(c.successors(), vec![Label(4)]);

    let r: InstructionIR = ReturnIR::new(Label(6)).into();

    assert!(r.successors().is_empty());
}

#[test]
fn resolve_match_terminal() {
    // Terminal match: empty successors → next = 0 in bytecode
    let m = MatchIR::terminal(Label(0));

    let mut map = BTreeMap::new();
    map.insert(Label(0), 1u16);

    let ctx = EmitContext::new(&|_| None, &|_| None);
    let bytes = m.resolve(&map, &ctx).expect("terminal match encodes");
    assert_eq!(bytes.len(), 8);

    assert_eq!(bytes[0] & 0xF, 0);
    assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 0);
}

#[test]
fn member_ref_resolution() {
    let parent_type = TypeId(20);

    let member = MemberRef::new(parent_type, 3);
    let get_member_base = |ty| if ty == parent_type { Some(50) } else { None };
    let ctx = EmitContext::new(&get_member_base, &|_| None);
    assert_eq!(member.resolve(&ctx), 53); // base 50 + relative 3
}

#[test]
fn effect_ir_resolution() {
    let parent_type = TypeId(10);

    let simple = EffectIR::simple(EffectOpcode::Node, 5);
    let simple_ctx = EmitContext::new(&|_| None, &|_| None);
    let resolved = simple.resolve(&simple_ctx);
    assert_eq!(resolved.opcode, EffectOpcode::Node);
    assert_eq!(resolved.payload, 5);

    let set_effect = EffectIR::with_member(EffectOpcode::Set, MemberRef::new(parent_type, 1));
    let get_member_base = |ty| if ty == parent_type { Some(50) } else { None };
    let set_ctx = EmitContext::new(&get_member_base, &|_| None);
    let resolved = set_effect.resolve(&set_ctx);
    assert_eq!(resolved.opcode, EffectOpcode::Set);
    assert_eq!(resolved.payload, 51); // base 50 + relative 1
}
