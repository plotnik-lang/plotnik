use super::{JournalEvent, MatchJournal};

#[test]
fn journal_events_remain_compact() {
    assert_eq!(std::mem::size_of::<JournalEvent<'_>>(), 48);
}

#[test]
fn output_events_exclude_inspection_events() {
    let mut journal = MatchJournal::new();
    journal.push(JournalEvent::SpanStart { id: 7, node: None });
    journal.push(JournalEvent::RecordOpen);
    journal.push(JournalEvent::SpanEnd(7));
    journal.push(JournalEvent::RecordClose);

    let events = journal.output_events();

    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], JournalEvent::RecordOpen));
    assert!(matches!(events[1], JournalEvent::RecordClose));
}
