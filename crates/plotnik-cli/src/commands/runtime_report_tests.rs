use plotnik_lib::RuntimeError;

use super::runtime_report::render_runtime_error;

#[test]
fn out_of_fuel_text_has_code_grouped_count_and_flag() {
    let msg = render_runtime_error(&RuntimeError::OutOfFuel(2_000_000), false);
    assert!(msg.contains("out of fuel"));
    assert!(msg.contains("[E-out-of-fuel]"));
    assert!(msg.contains("2,000,000 fuel units"));
    assert!(msg.contains("--fuel"));
    assert!(msg.contains("unbounded"));
    // Four lines: what / halted / why / how.
    assert_eq!(msg.lines().count(), 4);
}

#[test]
fn memory_limit_text_shows_usage_and_ceiling() {
    let msg = render_runtime_error(
        &RuntimeError::MemoryLimitExceeded {
            used: 80 << 20,
            limit: 64 << 20,
        },
        false,
    );
    assert!(msg.contains("memory limit exceeded"));
    assert!(msg.contains("[E-limit-memory]"));
    assert!(msg.contains("64.0 MiB")); // the ceiling
    assert!(msg.contains("80.0 MiB")); // actual usage at the trip point — the tunable number
    assert!(msg.contains("--max-memory"));
}

#[test]
fn json_form_is_compact_machine_readable() {
    let fuel = render_runtime_error(&RuntimeError::OutOfFuel(500), true);
    assert_eq!(
        fuel,
        r#"{"error":"limit-exceeded","code":"E-out-of-fuel","fuel_limit":500}"#
    );

    let mem = render_runtime_error(
        &RuntimeError::MemoryLimitExceeded {
            used: 2048,
            limit: 1024,
        },
        true,
    );
    assert_eq!(
        mem,
        r#"{"error":"limit-exceeded","code":"E-limit-memory","max_memory":1024,"used":2048}"#
    );
}

#[test]
fn non_limit_error_falls_back_to_display() {
    let err = RuntimeError::NoMatch;
    assert_eq!(render_runtime_error(&err, false), "error: no match found");
    assert!(render_runtime_error(&err, true).contains("\"runtime\""));
}
