use plotnik_lib::engine::RuntimeError;

use super::runtime_report::render_runtime_error;

#[test]
fn step_limit_text_has_code_grouped_count_and_flag() {
    let msg = render_runtime_error(&RuntimeError::StepLimitExceeded(2_000_000), false);
    assert!(msg.contains("step limit exceeded"));
    assert!(msg.contains("[E-limit-steps]"));
    assert!(msg.contains("2,000,000 steps"));
    assert!(msg.contains("--max-steps"));
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
    let steps = render_runtime_error(&RuntimeError::StepLimitExceeded(500), true);
    assert_eq!(
        steps,
        r#"{"error":"limit-exceeded","code":"E-limit-steps","max_steps":500}"#
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
