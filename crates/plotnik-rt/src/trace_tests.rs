use crate::{EffectLog, RuntimeEffect, TraceReader};

fn log(entries: Vec<RuntimeEffect<'static>>) -> EffectLog<'static> {
    let mut log = EffectLog::new();
    for entry in entries {
        log.push(entry);
    }
    log
}

#[test]
fn peek_set_sees_through_a_scalar() {
    let log = log(vec![
        RuntimeEffect::ScalarOpen,
        RuntimeEffect::BoolClose(true),
        RuntimeEffect::Set(7),
    ]);

    let t = TraceReader::new(&log, "");

    assert_eq!(t.peek_set(), 7);
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
fn absent_string_is_consumed_as_optional_null() {
    let log = log(vec![RuntimeEffect::ScalarOpen, RuntimeEffect::StrClose]);

    let mut t = TraceReader::new(&log, "");

    assert!(t.take_null());
    t.finish();
}

#[test]
fn peek_set_skips_a_balanced_composite() {
    // An inner Set(1) hides inside the struct value; the field's own Set(9)
    // is the first one at depth zero.
    let log = log(vec![
        RuntimeEffect::StructOpen,
        RuntimeEffect::Null,
        RuntimeEffect::Set(1),
        RuntimeEffect::StructClose,
        RuntimeEffect::Set(9),
    ]);

    let t = TraceReader::new(&log, "");

    assert_eq!(t.peek_set(), 9);
}

#[test]
fn peek_set_skips_an_empty_array() {
    // Two shape-identical empty-array prefixes are told apart only by the
    // member index behind them — the reader's dispatch relies on this.
    let log = log(vec![
        RuntimeEffect::ArrayOpen,
        RuntimeEffect::ArrayClose,
        RuntimeEffect::Set(3),
    ]);

    let t = TraceReader::new(&log, "");

    assert_eq!(t.peek_set(), 3);
}

#[test]
fn peek_set_answers_at_every_level_of_a_nested_value() {
    // A struct field value that is itself a struct: peeked at its open, the
    // answer is the *outer* Set; peeked inside, the inner field's own Set.
    let log = log(vec![
        RuntimeEffect::StructOpen,
        RuntimeEffect::Null,
        RuntimeEffect::Set(1),
        RuntimeEffect::StructClose,
        RuntimeEffect::Set(9),
    ]);

    let mut t = TraceReader::new(&log, "");

    assert_eq!(t.peek_set(), 9);
    t.expect_struct_open();
    assert_eq!(t.peek_set(), 1);
    assert!(t.take_null());
    assert_eq!(t.expect_set(), 1);
    t.expect_struct_close();
    assert_eq!(t.expect_set(), 9);
    t.finish();
}

#[test]
fn take_null_consumes_only_null() {
    let log = log(vec![RuntimeEffect::Null, RuntimeEffect::Set(0)]);

    let mut t = TraceReader::new(&log, "");

    assert!(t.take_null());
    assert!(!t.take_null());
    assert_eq!(t.expect_set(), 0);
    t.finish();
}

#[test]
#[should_panic(expected = "left unread")]
fn finish_rejects_leftovers() {
    let log = log(vec![RuntimeEffect::Null]);

    let t = TraceReader::new(&log, "");

    t.finish();
}
