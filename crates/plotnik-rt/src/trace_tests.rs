use crate::{JournalEvent, MatchJournal, TraceReader};

fn journal(entries: Vec<JournalEvent<'static>>) -> MatchJournal<'static> {
    let mut journal = MatchJournal::new();
    for entry in entries {
        journal.push(entry);
    }
    journal
}

#[test]
fn peek_record_set_sees_through_a_scalar() {
    let journal = journal(vec![
        JournalEvent::ScalarOpen,
        JournalEvent::BoolClose(true),
        JournalEvent::RecordSet(7),
    ]);

    let t = TraceReader::new(&journal, "");

    assert_eq!(t.peek_record_set(), 7);
}

#[test]
fn bool_reader_consumes_one_balanced_scalar() {
    let journal = journal(vec![
        JournalEvent::ScalarOpen,
        JournalEvent::BoolClose(false),
    ]);

    let mut t = TraceReader::new(&journal, "");

    assert!(!t.expect_bool());
    t.finish();
}

#[test]
fn absent_string_is_consumed_as_an_option() {
    let journal = journal(vec![JournalEvent::ScalarOpen, JournalEvent::StrClose]);

    let mut t = TraceReader::new(&journal, "");

    assert!(t.take_absent());
    t.finish();
}

#[test]
fn peek_record_set_skips_a_balanced_composite() {
    // An inner RecordSet(1) hides inside the record value; the field's own
    // RecordSet(9)
    // is the first one at depth zero.
    let journal = journal(vec![
        JournalEvent::RecordOpen,
        JournalEvent::Absent,
        JournalEvent::RecordSet(1),
        JournalEvent::RecordClose,
        JournalEvent::RecordSet(9),
    ]);

    let t = TraceReader::new(&journal, "");

    assert_eq!(t.peek_record_set(), 9);
}

#[test]
fn peek_record_set_skips_an_empty_list() {
    // Two shape-identical empty-list prefixes are told apart only by the
    // member index behind them — the reader's dispatch relies on this.
    let journal = journal(vec![
        JournalEvent::ListOpen,
        JournalEvent::ListClose,
        JournalEvent::RecordSet(3),
    ]);

    let t = TraceReader::new(&journal, "");

    assert_eq!(t.peek_record_set(), 3);
}

#[test]
fn peek_record_set_answers_at_every_level_of_a_nested_value() {
    // A record field value that is itself a record: peeked at its open, the
    // answer is the outer RecordSet; peeked inside, the inner field's own
    // RecordSet.
    let journal = journal(vec![
        JournalEvent::RecordOpen,
        JournalEvent::Absent,
        JournalEvent::RecordSet(1),
        JournalEvent::RecordClose,
        JournalEvent::RecordSet(9),
    ]);

    let mut t = TraceReader::new(&journal, "");

    assert_eq!(t.peek_record_set(), 9);
    t.expect_record_open();
    assert_eq!(t.peek_record_set(), 1);
    assert!(t.take_absent());
    assert_eq!(t.expect_record_set(), 1);
    t.expect_record_close();
    assert_eq!(t.expect_record_set(), 9);
    t.finish();
}

#[test]
fn take_absent_consumes_only_absence() {
    let journal = journal(vec![JournalEvent::Absent, JournalEvent::RecordSet(0)]);

    let mut t = TraceReader::new(&journal, "");

    assert!(t.take_absent());
    assert!(!t.take_absent());
    assert_eq!(t.expect_record_set(), 0);
    t.finish();
}

#[test]
#[should_panic(expected = "left unread")]
fn finish_rejects_leftovers() {
    let journal = journal(vec![JournalEvent::Absent]);

    let t = TraceReader::new(&journal, "");

    t.finish();
}
