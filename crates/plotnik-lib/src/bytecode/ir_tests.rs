use std::collections::BTreeMap;
use std::num::NonZeroU16;

use plotnik_core::Symbol;

use super::effects::EffectOpcode;
use super::ir::{CallIR, EffectIR, InstructionIR, Label, MatchIR, MemberRef, ReturnIR};
use super::nav::Nav;
use crate::analyze::type_check::TypeId;

#[test]
fn match_ir_size_match8() {
    let m = MatchIR::at(Label(0))
        .nav(Nav::Down)
        .node_type(NonZeroU16::new(10))
        .next(Label(1));

    assert_eq!(m.size(), 8);
}

#[test]
fn match_ir_size_extended() {
    let m = MatchIR::at(Label(0))
        .nav(Nav::Down)
        .node_type(NonZeroU16::new(10))
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

    let bytes = m.resolve(&map, |_, _| None, |_| None);
    assert_eq!(bytes.len(), 8);

    // Verify opcode is Match8 (0x0)
    assert_eq!(bytes[0] & 0xF, 0);
    // Verify next is terminal (0)
    assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 0);
}

#[test]
fn member_ref_resolution() {
    // Create test symbols (these are just integer handles)
    let field_name = Symbol::from_raw(100);
    let field_type = TypeId(10);
    let parent_type = TypeId(20);

    // Test absolute reference
    let abs = MemberRef::absolute(42);
    assert_eq!(abs.resolve(|_, _| None, |_| None), 42);

    // Test deferred reference with lookup (struct field)
    let deferred = MemberRef::deferred(field_name, field_type);
    assert_eq!(
        deferred.resolve(
            |name, ty| {
                if name == field_name && ty == field_type {
                    Some(77)
                } else {
                    None
                }
            },
            |_| None
        ),
        77
    );

    // Test deferred reference with no match (defaults to 0)
    assert_eq!(deferred.resolve(|_, _| None, |_| None), 0);

    // Test deferred by index reference (enum variant)
    let by_index = MemberRef::deferred_by_index(parent_type, 3);
    assert_eq!(
        by_index.resolve(
            |_, _| None,
            |ty| if ty == parent_type { Some(50) } else { None }
        ),
        53 // base 50 + relative 3
    );
}

#[test]
fn effect_ir_resolution() {
    let field_name = Symbol::from_raw(200);
    let field_type = TypeId(10);

    // Simple effect without member ref
    let simple = EffectIR::simple(EffectOpcode::Node, 5);
    let resolved = simple.resolve(|_, _| None, |_| None);
    assert_eq!(resolved.opcode, EffectOpcode::Node);
    assert_eq!(resolved.payload, 5);

    // Effect with deferred member ref
    let set_effect = EffectIR::with_member(
        EffectOpcode::Set,
        MemberRef::deferred(field_name, field_type),
    );
    let resolved = set_effect.resolve(
        |name, ty| {
            if name == field_name && ty == field_type {
                Some(51)
            } else {
                None
            }
        },
        |_| None,
    );
    assert_eq!(resolved.opcode, EffectOpcode::Set);
    assert_eq!(resolved.payload, 51);
}
