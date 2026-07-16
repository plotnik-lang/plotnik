use crate::{JournalEvent, MatchJournal, ResultDecoder};

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

    let decoder = ResultDecoder::new(journal.output_events(), "");

    assert_eq!(decoder.peek_record_set(), 7);
}

#[test]
fn bool_decoder_consumes_one_balanced_scalar() {
    let journal = journal(vec![
        JournalEvent::ScalarOpen,
        JournalEvent::BoolClose(false),
    ]);

    let mut decoder = ResultDecoder::new(journal.output_events(), "");

    assert!(!decoder.expect_bool());
    decoder.finish();
}

#[test]
fn absent_string_is_consumed_as_an_option() {
    let journal = journal(vec![JournalEvent::ScalarOpen, JournalEvent::TextClose]);

    let mut decoder = ResultDecoder::new(journal.output_events(), "");

    assert!(decoder.take_absent());
    decoder.finish();
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

    let decoder = ResultDecoder::new(journal.output_events(), "");

    assert_eq!(decoder.peek_record_set(), 9);
}

#[test]
fn peek_record_set_skips_an_empty_list() {
    // Two shape-identical empty-list prefixes are told apart only by the
    // member index behind them — the decoder's dispatch relies on this.
    let journal = journal(vec![
        JournalEvent::ListOpen,
        JournalEvent::ListClose,
        JournalEvent::RecordSet(3),
    ]);

    let decoder = ResultDecoder::new(journal.output_events(), "");

    assert_eq!(decoder.peek_record_set(), 3);
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

    let mut decoder = ResultDecoder::new(journal.output_events(), "");

    assert_eq!(decoder.peek_record_set(), 9);
    decoder.expect_record_open();
    assert_eq!(decoder.peek_record_set(), 1);
    assert!(decoder.take_absent());
    assert_eq!(decoder.expect_record_set(), 1);
    decoder.expect_record_close();
    assert_eq!(decoder.expect_record_set(), 9);
    decoder.finish();
}

#[test]
fn take_absent_consumes_only_absence() {
    let journal = journal(vec![JournalEvent::Absent, JournalEvent::RecordSet(0)]);

    let mut decoder = ResultDecoder::new(journal.output_events(), "");

    assert!(decoder.take_absent());
    assert!(!decoder.take_absent());
    assert_eq!(decoder.expect_record_set(), 0);
    decoder.finish();
}

#[test]
#[should_panic(expected = "left unread")]
fn finish_rejects_leftovers() {
    let journal = journal(vec![JournalEvent::Absent]);

    let decoder = ResultDecoder::new(journal.output_events(), "");

    decoder.finish();
}
