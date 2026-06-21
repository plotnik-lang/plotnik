use clap::Command;
use plotnik_lib::{Limit, RuntimeLimitSpec};

use super::limits::{
    limits_preset_arg, max_memory_arg, max_steps_arg, parse_memory, parse_size, parse_steps,
    resolve_limit_spec,
};

fn spec_from(args: &[&str]) -> RuntimeLimitSpec {
    let cmd = Command::new("t")
        .arg(max_steps_arg())
        .arg(max_memory_arg())
        .arg(limits_preset_arg());
    let argv = std::iter::once("t").chain(args.iter().copied());
    let m = cmd.try_get_matches_from(argv).expect("args parse");
    resolve_limit_spec(&m)
}

#[test]
fn parse_size_bytes_and_binary_units() {
    assert_eq!(parse_size("0"), Ok(0));
    assert_eq!(parse_size("512"), Ok(512));
    assert_eq!(parse_size("512B"), Ok(512));
    assert_eq!(parse_size("1KiB"), Ok(1024));
    assert_eq!(parse_size("64MiB"), Ok(64 << 20));
    assert_eq!(parse_size("2GiB"), Ok(2 << 30));
}

#[test]
fn parse_size_is_lenient_on_case_and_spacing() {
    assert_eq!(parse_size("64 MiB"), Ok(64 << 20));
    assert_eq!(parse_size("  64mib "), Ok(64 << 20));
    assert_eq!(parse_size("1gib"), Ok(1 << 30));
}

#[test]
fn parse_size_rejects_si_units() {
    assert!(parse_size("64MB").is_err());
    assert!(parse_size("64KB").is_err());
    assert!(parse_size("1GB").is_err());
}

#[test]
fn parse_size_rejects_fractional_and_garbage() {
    assert!(parse_size("1.5MiB").is_err());
    assert!(parse_size("MiB").is_err());
    assert!(parse_size("12ZiB").is_err());
    assert!(parse_size("").is_err());
}

#[test]
fn parse_steps_keywords_and_numbers() {
    assert_eq!(parse_steps("auto"), Ok(Limit::Auto));
    assert_eq!(parse_steps("AUTO"), Ok(Limit::Auto));
    assert_eq!(parse_steps("unbounded"), Ok(Limit::Unbounded));
    assert_eq!(parse_steps("none"), Ok(Limit::Unbounded));
    assert_eq!(parse_steps("5000"), Ok(Limit::Of(5000)));
    assert!(parse_steps("lots").is_err());
}

#[test]
fn parse_memory_keywords_and_sizes() {
    assert_eq!(parse_memory("auto"), Ok(Limit::Auto));
    assert_eq!(parse_memory("unbounded"), Ok(Limit::Unbounded));
    assert_eq!(parse_memory("64MiB"), Ok(Limit::Of(64 << 20)));
}

#[test]
fn default_spec_is_auto_auto() {
    let spec = spec_from(&[]);
    assert_eq!(spec.steps, Limit::Auto);
    assert_eq!(spec.memory, Limit::Auto);
}

#[test]
fn explicit_overrides_apply_per_field() {
    let spec = spec_from(&["--max-steps", "5", "--max-memory", "32MiB"]);
    assert_eq!(spec.steps, Limit::Of(5));
    assert_eq!(spec.memory, Limit::Of(32 << 20));
}

#[test]
fn preset_sets_the_baseline_for_both() {
    let spec = spec_from(&["--limits", "unbounded"]);
    assert_eq!(spec.steps, Limit::Unbounded);
    assert_eq!(spec.memory, Limit::Unbounded);
}

#[test]
fn per_field_override_beats_preset_regardless_of_order() {
    // The plan's canonical case: unbounded everywhere except steps.
    let forward = spec_from(&["--limits", "unbounded", "--max-steps", "5"]);
    let reversed = spec_from(&["--max-steps", "5", "--limits", "unbounded"]);
    for spec in [forward, reversed] {
        assert_eq!(spec.steps, Limit::Of(5));
        assert_eq!(spec.memory, Limit::Unbounded);
    }
}
