use super::capture::CaptureEffects;
use plotnik_bytecode::EffectOpcode;
use crate::bytecode::{EffectIR, MemberRef};

#[test]
fn nest_scope_preserves_outer_and_nests_inner() {
    let outer = CaptureEffects::new(vec![EffectIR::start_obj()], vec![EffectIR::end_obj()]);

    let result = outer.nest_scope(EffectIR::start_enum(), EffectIR::end_enum());

    assert_eq!(result.pre.len(), 2);
    assert_eq!(result.pre[0].opcode, EffectOpcode::Obj);
    assert_eq!(result.pre[1].opcode, EffectOpcode::Enum);

    assert_eq!(result.post.len(), 2);
    assert_eq!(result.post[0].opcode, EffectOpcode::EndEnum);
    assert_eq!(result.post[1].opcode, EffectOpcode::EndObj);
}

#[test]
fn with_pre_values_appends_after_scope_opens() {
    let outer = CaptureEffects::new_pre(vec![EffectIR::start_obj()]);

    let result = outer.with_pre_values(vec![
        EffectIR::null(),
        EffectIR::with_member(EffectOpcode::Set, MemberRef::absolute(0)),
    ]);

    assert_eq!(result.pre.len(), 3);
    assert_eq!(result.pre[0].opcode, EffectOpcode::Obj);
    assert_eq!(result.pre[1].opcode, EffectOpcode::Null);
    assert_eq!(result.pre[2].opcode, EffectOpcode::Set);
}

#[test]
fn with_post_values_prepends_before_scope_closes() {
    let outer = CaptureEffects::new_post(vec![EffectIR::end_obj()]);

    let result = outer.with_post_values(vec![
        EffectIR::node(),
        EffectIR::with_member(EffectOpcode::Set, MemberRef::absolute(0)),
    ]);

    assert_eq!(result.post.len(), 3);
    assert_eq!(result.post[0].opcode, EffectOpcode::Node);
    assert_eq!(result.post[1].opcode, EffectOpcode::Set);
    assert_eq!(result.post[2].opcode, EffectOpcode::EndObj);
}

#[test]
#[should_panic(expected = "nest_scope expects scope-opening effect")]
fn nest_scope_rejects_non_scope_open() {
    let outer = CaptureEffects::default();
    outer.nest_scope(EffectIR::node(), EffectIR::end_obj());
}

#[test]
#[should_panic(expected = "nest_scope expects scope-closing effect")]
fn nest_scope_rejects_non_scope_close() {
    let outer = CaptureEffects::default();
    outer.nest_scope(EffectIR::start_obj(), EffectIR::node());
}
