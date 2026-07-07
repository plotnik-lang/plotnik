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
    let log = log(vec![RuntimeEffect::Null, RuntimeEffect::Set(7)]);

    let t = TraceReader::new(&log);

    assert_eq!(t.peek_set(), 7);
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

    let t = TraceReader::new(&log);

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

    let t = TraceReader::new(&log);

    assert_eq!(t.peek_set(), 3);
}

#[test]
fn take_null_consumes_only_null() {
    let log = log(vec![RuntimeEffect::Null, RuntimeEffect::Set(0)]);

    let mut t = TraceReader::new(&log);

    assert!(t.take_null());
    assert!(!t.take_null());
    assert_eq!(t.expect_set(), 0);
    t.finish();
}

#[test]
#[should_panic(expected = "left unread")]
fn finish_rejects_leftovers() {
    let log = log(vec![RuntimeEffect::Null]);

    let t = TraceReader::new(&log);

    t.finish();
}
