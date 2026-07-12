use super::RuntimeEffect;

#[test]
fn runtime_effects_remain_compact() {
    assert_eq!(std::mem::size_of::<RuntimeEffect<'_>>(), 48);
}
