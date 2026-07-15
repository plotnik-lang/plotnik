use super::capture::CaptureEffects;
use crate::bytecode::EffectKind;
use crate::compiler::ids::ResultMemberId;
use crate::compiler::lower::ir::EffectIR;

#[test]
fn nest_scope_preserves_outer_and_nests_inner() {
    let outer = CaptureEffects::new(
        vec![EffectIR::record_open()],
        vec![EffectIR::record_close()],
    );

    let result = outer.nest_scope(
        EffectIR::with_member(EffectKind::VariantOpen, ResultMemberId::from_raw(0)),
        EffectIR::end_variant(),
    );

    assert_eq!(result.pre.len(), 2);
    assert_eq!(result.pre[0].kind(), EffectKind::RecordOpen);
    assert_eq!(result.pre[1].kind(), EffectKind::VariantOpen);

    assert_eq!(result.post.len(), 2);
    assert_eq!(result.post[0].kind(), EffectKind::VariantClose);
    assert_eq!(result.post[1].kind(), EffectKind::RecordClose);
}

#[test]
fn with_pre_values_appends_after_scope_opens() {
    let outer = CaptureEffects::new(vec![EffectIR::record_open()], vec![]);

    let result = outer.with_pre_values(vec![
        EffectIR::absent(),
        EffectIR::with_member(EffectKind::RecordSet, ResultMemberId::from_raw(0)),
    ]);

    assert_eq!(result.pre.len(), 3);
    assert_eq!(result.pre[0].kind(), EffectKind::RecordOpen);
    assert_eq!(result.pre[1].kind(), EffectKind::Absent);
    assert_eq!(result.pre[2].kind(), EffectKind::RecordSet);
}

#[test]
fn with_post_values_prepends_before_scope_closes() {
    let outer = CaptureEffects::new_post(vec![EffectIR::record_close()]);

    let result = outer.with_post_values(vec![
        EffectIR::node(),
        EffectIR::with_member(EffectKind::RecordSet, ResultMemberId::from_raw(0)),
    ]);

    assert_eq!(result.post.len(), 3);
    assert_eq!(result.post[0].kind(), EffectKind::Node);
    assert_eq!(result.post[1].kind(), EffectKind::RecordSet);
    assert_eq!(result.post[2].kind(), EffectKind::RecordClose);
}

#[test]
#[should_panic(expected = "nest_scope expects scope-opening effect")]
fn nest_scope_rejects_non_scope_open() {
    let outer = CaptureEffects::default();
    outer.nest_scope(EffectIR::node(), EffectIR::record_close());
}

#[test]
#[should_panic(expected = "nest_scope expects scope-closing effect")]
fn nest_scope_rejects_non_scope_close() {
    let outer = CaptureEffects::default();
    outer.nest_scope(EffectIR::record_open(), EffectIR::node());
}
