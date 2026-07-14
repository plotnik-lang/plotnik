use clap::Command;
use plotnik_lib::{Limit, RuntimeLimitSpec};

use super::limits::{
    fuel_arg, limits_preset_arg, max_memory_arg, parse_fuel, parse_memory, parse_size,
    resolve_limit_spec,
};

fn spec_from(args: &[&str]) -> RuntimeLimitSpec {
    let cmd = Command::new("t")
        .arg(fuel_arg())
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
fn parse_fuel_keywords_and_numbers() {
    assert_eq!(parse_fuel("auto"), Ok(Limit::Auto));
    assert_eq!(parse_fuel("AUTO"), Ok(Limit::Auto));
    assert_eq!(parse_fuel("unbounded"), Ok(Limit::Unbounded));
    assert_eq!(parse_fuel("5000"), Ok(Limit::Of(5000)));
    assert!(parse_fuel("lots").is_err());
    // `unbounded` is the one opt-out spelling — no `none` synonym, matching `--limits`.
    assert!(parse_fuel("none").is_err());
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
    assert_eq!(spec.fuel_limit, Limit::Auto);
    assert_eq!(spec.memory, Limit::Auto);
}

#[test]
fn explicit_overrides_apply_per_field() {
    let spec = spec_from(&["--fuel", "5", "--max-memory", "32MiB"]);
    assert_eq!(spec.fuel_limit, Limit::Of(5));
    assert_eq!(spec.memory, Limit::Of(32 << 20));
}

#[test]
fn preset_sets_the_baseline_for_both() {
    let spec = spec_from(&["--limits", "unbounded"]);
    assert_eq!(spec.fuel_limit, Limit::Unbounded);
    assert_eq!(spec.memory, Limit::Unbounded);
}

#[test]
fn per_field_override_beats_preset_regardless_of_order() {
    // The canonical case: unbounded runtime limits except fuel.
    let forward = spec_from(&["--limits", "unbounded", "--fuel", "5"]);
    let reversed = spec_from(&["--fuel", "5", "--limits", "unbounded"]);
    for spec in [forward, reversed] {
        assert_eq!(spec.fuel_limit, Limit::Of(5));
        assert_eq!(spec.memory, Limit::Unbounded);
    }
}
