//! Runtime execution-limit flags and their parsing.
//!
//! Three knobs feed one [`RuntimeLimitSpec`]:
//!   - `--max-steps  <auto|unbounded|N>`     work ceiling
//!   - `--max-memory <auto|unbounded|SIZE>`  live-heap ceiling (binary units)
//!   - `--limits     <auto|unbounded>`       preset baseline for every resource
//!
//! Precedence is order-independent: `--limits` sets the baseline and an explicit
//! `--max-*` overrides that one resource. So `--limits unbounded --max-steps 5`
//! means "unbounded except steps = 5", regardless of flag order.

use clap::{Arg, ArgMatches};
use plotnik_lib::{Limit, RuntimeLimitSpec};

pub fn max_steps_arg() -> Arg {
    Arg::new("max_steps")
        .long("max-steps")
        .value_name("LIMIT")
        .value_parser(parse_steps)
        .help("Work limit: a step count, 'auto' (size-based), or 'unbounded'")
}

pub fn max_memory_arg() -> Arg {
    Arg::new("max_memory")
        .long("max-memory")
        .value_name("LIMIT")
        .value_parser(parse_memory)
        .help("Memory limit: a size (e.g. 64MiB), 'auto' (size-based), or 'unbounded'")
}

pub fn limits_preset_arg() -> Arg {
    Arg::new("limits")
        .long("limits")
        .value_name("PRESET")
        .value_parser(["auto", "unbounded"])
        .help("Limit preset for every resource: 'auto' (default) or 'unbounded'")
}

/// Combine the `--limits` preset baseline with per-resource `--max-*` overrides.
/// Reads the final parsed values, so flag order does not matter.
pub fn resolve_limit_spec(m: &ArgMatches) -> RuntimeLimitSpec {
    let base = match m.get_one::<String>("limits").map(String::as_str) {
        Some("unbounded") => RuntimeLimitSpec {
            steps: Limit::Unbounded,
            memory: Limit::Unbounded,
        },
        // `auto` and the absent flag both mean the size-based default; the
        // value_parser admits no other preset.
        _ => RuntimeLimitSpec::default(),
    };
    RuntimeLimitSpec {
        steps: m
            .get_one::<Limit>("max_steps")
            .copied()
            .unwrap_or(base.steps),
        memory: m
            .get_one::<Limit>("max_memory")
            .copied()
            .unwrap_or(base.memory),
    }
}

/// `auto` | `unbounded` | a non-negative step count.
pub(crate) fn parse_steps(raw: &str) -> Result<Limit, String> {
    if let Some(limit) = keyword(raw) {
        return Ok(limit);
    }
    raw.trim().parse::<u64>().map(Limit::Of).map_err(|_| {
        format!("invalid step limit '{raw}': expected a number, 'auto', or 'unbounded'")
    })
}

/// `auto` | `unbounded` | a binary size (`64MiB`, `512KiB`, `1GiB`, or bytes).
pub(crate) fn parse_memory(raw: &str) -> Result<Limit, String> {
    if let Some(limit) = keyword(raw) {
        return Ok(limit);
    }
    parse_size(raw).map(Limit::Of)
}

/// The `auto`/`unbounded` keywords shared by both numeric knobs. These are the
/// only spellings, matching `--limits` — there is deliberately no `none` synonym.
fn keyword(raw: &str) -> Option<Limit> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(Limit::Auto),
        "unbounded" => Some(Limit::Unbounded),
        _ => None,
    }
}

/// Parse a byte size with binary units only: a bare integer is bytes; `KiB`,
/// `MiB`, `GiB` (case-insensitive) scale by 1024^n. SI units (`KB`/`MB`/`GB`)
/// are rejected as ambiguous rather than silently reinterpreted.
pub(crate) fn parse_size(raw: &str) -> Result<u64, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("invalid size: empty".to_string());
    }
    if s.contains('.') {
        return Err(format!(
            "invalid size '{raw}': fractional sizes are unsupported, use whole KiB/MiB/GiB"
        ));
    }

    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (number, unit) = s.split_at(split);
    if number.is_empty() {
        return Err(format!("invalid size '{raw}': expected a number"));
    }
    let value: u64 = number
        .parse()
        .map_err(|_| format!("invalid size '{raw}': number too large"))?;

    let unit = unit.trim();
    let multiplier: u64 = if unit.is_empty() || unit.eq_ignore_ascii_case("B") {
        1
    } else if unit.eq_ignore_ascii_case("KiB") {
        1 << 10
    } else if unit.eq_ignore_ascii_case("MiB") {
        1 << 20
    } else if unit.eq_ignore_ascii_case("GiB") {
        1 << 30
    } else if matches!(
        unit.to_ascii_uppercase().as_str(),
        "KB" | "MB" | "GB" | "K" | "M" | "G"
    ) {
        return Err(format!(
            "invalid size '{raw}': use binary units (KiB, MiB, GiB), not '{unit}'"
        ));
    } else {
        return Err(format!("invalid size '{raw}': unknown unit '{unit}'"));
    };

    value
        .checked_mul(multiplier)
        .ok_or_else(|| format!("invalid size '{raw}': overflows u64"))
}
