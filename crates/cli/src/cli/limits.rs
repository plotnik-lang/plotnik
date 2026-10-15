use clap::Arg;
use plotnik_compiler::{Limit, RuntimeLimitSpec};

pub fn runtime_limits(m: &clap::ArgMatches) -> RuntimeLimitSpec {
    let preset = match m.get_one::<String>("limits").map(String::as_str) {
        Some("unbounded") => Limit::Unbounded,
        _ => Limit::Auto,
    };
    let limit = |name: &str, parse: fn(&str) -> Result<u64, String>| match m
        .get_one::<String>(name)
        .map(|s| s.trim().to_ascii_lowercase())
    {
        None => preset,
        Some(raw) if raw == "auto" => Limit::Auto,
        Some(raw) if raw == "unbounded" => Limit::Unbounded,
        Some(raw) => Limit::Of(parse(&raw).expect("clap validated the runtime limit")),
    };
    RuntimeLimitSpec {
        fuel_limit: limit("fuel", |s| s.parse::<u64>().map_err(|e| e.to_string())),
        memory: limit("max_memory", parse_size),
    }
}

pub fn fuel_arg() -> Arg {
    Arg::new("fuel")
        .long("fuel")
        .value_name("LIMIT")
        .value_parser(parse_fuel)
        .help("Matcher work budget: a fuel amount, 'auto' (size-based), or 'unbounded'")
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
        .help("Runtime limit preset: 'auto' (default) or 'unbounded'")
}

pub(crate) fn parse_fuel(raw: &str) -> Result<String, String> {
    if keyword(raw) {
        return Ok(raw.to_string());
    }
    raw.trim()
        .parse::<u64>()
        .map(|_| raw.to_string())
        .map_err(|_| {
            format!("invalid fuel limit '{raw}': expected a number, 'auto', or 'unbounded'")
        })
}

pub(crate) fn parse_memory(raw: &str) -> Result<String, String> {
    if keyword(raw) {
        return Ok(raw.to_string());
    }
    parse_size(raw).map(|_| raw.to_string())
}

fn keyword(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "auto" | "unbounded"
    )
}

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
