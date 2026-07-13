use crate::{EffectLog, RuntimeEffect, TraceReader};

fn log(entries: Vec<RuntimeEffect<'static>>) -> EffectLog<'static> {
    let mut log = EffectLog::new();
    for entry in entries {
        log.push(entry);
    }
    log
}

#[test]
fn peek_record_set_sees_through_a_scalar() {
    let log = log(vec![
        RuntimeEffect::ScalarOpen,
        RuntimeEffect::BoolClose(true),
        RuntimeEffect::RecordSet(7),
    ]);

    let t = TraceReader::new(&log, "");

    assert_eq!(t.peek_record_set(), 7);
}

#[test]
fn bool_reader_consumes_one_balanced_scalar() {
    let log = log(vec![
        RuntimeEffect::ScalarOpen,
        RuntimeEffect::BoolClose(false),
    ]);

    let mut t = TraceReader::new(&log, "");

    assert!(!t.expect_bool());
    t.finish();
}

#[test]
fn absent_string_is_consumed_as_an_option() {
    let log = log(vec![RuntimeEffect::ScalarOpen, RuntimeEffect::StrClose]);

    let mut t = TraceReader::new(&log, "");

    assert!(t.take_absent());
    t.finish();
}

#[test]
fn peek_record_set_skips_a_balanced_composite() {
    // An inner RecordSet(1) hides inside the record value; the field's own
    // RecordSet(9)
    // is the first one at depth zero.
    let log = log(vec![
        RuntimeEffect::RecordOpen,
        RuntimeEffect::Absent,
        RuntimeEffect::RecordSet(1),
        RuntimeEffect::RecordClose,
        RuntimeEffect::RecordSet(9),
    ]);

    let t = TraceReader::new(&log, "");

    assert_eq!(t.peek_record_set(), 9);
}

#[test]
fn peek_record_set_skips_an_empty_list() {
    // Two shape-identical empty-list prefixes are told apart only by the
    // member index behind them — the reader's dispatch relies on this.
    let log = log(vec![
        RuntimeEffect::ListOpen,
        RuntimeEffect::ListClose,
        RuntimeEffect::RecordSet(3),
    ]);

    let t = TraceReader::new(&log, "");

    assert_eq!(t.peek_record_set(), 3);
}

#[test]
fn peek_record_set_answers_at_every_level_of_a_nested_value() {
    // A record field value that is itself a record: peeked at its open, the
    // answer is the outer RecordSet; peeked inside, the inner field's own
    // RecordSet.
    let log = log(vec![
        RuntimeEffect::RecordOpen,
        RuntimeEffect::Absent,
        RuntimeEffect::RecordSet(1),
        RuntimeEffect::RecordClose,
        RuntimeEffect::RecordSet(9),
    ]);

    let mut t = TraceReader::new(&log, "");

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
    let log = log(vec![RuntimeEffect::Absent, RuntimeEffect::RecordSet(0)]);

    let mut t = TraceReader::new(&log, "");

    assert!(t.take_absent());
    assert!(!t.take_absent());
    assert_eq!(t.expect_record_set(), 0);
    t.finish();
}

#[test]
#[should_panic(expected = "left unread")]
fn finish_rejects_leftovers() {
    let log = log(vec![RuntimeEffect::Absent]);

    let t = TraceReader::new(&log, "");

    t.finish();
}
