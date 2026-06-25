use super::capture::CaptureEffects;
use crate::bytecode::EffectKind;
use crate::compiler::ids::TypeId;
use crate::compiler::lower::ir::{EffectIR, MemberRef};

#[test]
fn nest_scope_preserves_outer_and_nests_inner() {
    let outer = CaptureEffects::new(vec![EffectIR::start_struct()], vec![EffectIR::end_struct()]);

    let result = outer.nest_scope(
        EffectIR::with_member(EffectKind::EnumOpen, MemberRef::new(TypeId(0), 0)),
        EffectIR::end_enum(),
    );

    assert_eq!(result.pre.len(), 2);
    assert_eq!(result.pre[0].kind(), EffectKind::StructOpen);
    assert_eq!(result.pre[1].kind(), EffectKind::EnumOpen);

    assert_eq!(result.post.len(), 2);
    assert_eq!(result.post[0].kind(), EffectKind::EnumClose);
    assert_eq!(result.post[1].kind(), EffectKind::StructClose);
}

#[test]
fn with_pre_values_appends_after_scope_opens() {
    let outer = CaptureEffects::new(vec![EffectIR::start_struct()], vec![]);

    let result = outer.with_pre_values(vec![
        EffectIR::null(),
        EffectIR::with_member(EffectKind::Set, MemberRef::new(TypeId(0), 0)),
    ]);

    assert_eq!(result.pre.len(), 3);
    assert_eq!(result.pre[0].kind(), EffectKind::StructOpen);
    assert_eq!(result.pre[1].kind(), EffectKind::Null);
    assert_eq!(result.pre[2].kind(), EffectKind::Set);
}

#[test]
fn with_post_values_prepends_before_scope_closes() {
    let outer = CaptureEffects::new_post(vec![EffectIR::end_struct()]);

    let result = outer.with_post_values(vec![
        EffectIR::node(),
        EffectIR::with_member(EffectKind::Set, MemberRef::new(TypeId(0), 0)),
    ]);

    assert_eq!(result.post.len(), 3);
    assert_eq!(result.post[0].kind(), EffectKind::Node);
    assert_eq!(result.post[1].kind(), EffectKind::Set);
    assert_eq!(result.post[2].kind(), EffectKind::StructClose);
}

#[test]
#[should_panic(expected = "nest_scope expects scope-opening effect")]
fn nest_scope_rejects_non_scope_open() {
    let outer = CaptureEffects::default();
    outer.nest_scope(EffectIR::node(), EffectIR::end_struct());
}

#[test]
#[should_panic(expected = "nest_scope expects scope-closing effect")]
fn nest_scope_rejects_non_scope_close() {
    let outer = CaptureEffects::default();
    outer.nest_scope(EffectIR::start_struct(), EffectIR::node());
}
