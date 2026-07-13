use super::JournalEvent;

#[test]
fn runtime_effects_remain_compact() {
    assert_eq!(std::mem::size_of::<JournalEvent<'_>>(), 48);
}
