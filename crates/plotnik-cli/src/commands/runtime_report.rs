//! CLI presentation for VM runtime errors.
//!
//! `RuntimeError` is a plain library enum with no `.ptk` source span, so it is
//! deliberately *not* routed through the diagnostics engine (which requires a
//! span and a `SourceMap` already dropped before execution). Instead the limit
//! errors get a four-line "what / halted / why / how" message with a stable
//! `E-limit-*` code; other variants fall back to their `Display`. `NoMatch` is a
//! domain answer the callers handle (exit 1), not a failure rendered here.

use plotnik_lib::engine::RuntimeError;

/// Render a runtime error for stderr, or as a one-line JSON object when `json`.
pub fn render_runtime_error(error: &RuntimeError, json: bool) -> String {
    match error {
        RuntimeError::StepLimitExceeded(limit) => render_limit(
            LimitReport {
                code: "E-limit-steps",
                title: "step limit exceeded",
                ceiling: format!("{} steps", group_thousands(*limit)),
                // Steps trip exactly at the ceiling, so usage adds nothing.
                used: None,
                why: "the query did more work than the step limit allows — usually \
                      catastrophic backtracking, or an unusually large input",
                flag: "--max-steps",
                json_key: "max_steps",
                json_value: limit.to_string(),
            },
            json,
        ),
        RuntimeError::MemoryLimitExceeded { used, limit } => render_limit(
            LimitReport {
                code: "E-limit-memory",
                title: "memory limit exceeded",
                ceiling: human_size(*limit),
                // Arenas grow geometrically, so usage overshoots the ceiling;
                // reporting it is what makes a new `--max-memory` computable.
                used: Some(Usage {
                    human: human_size(*used),
                    bytes: *used,
                }),
                why: "the query's live state outgrew the memory limit — usually \
                      catastrophic backtracking, or an unusually large input",
                flag: "--max-memory",
                json_key: "max_memory",
                json_value: limit.to_string(),
            },
            json,
        ),
        other if json => format!(
            r#"{{"error":"runtime","message":{}}}"#,
            json_quote(&other.to_string())
        ),
        other => format!("error: {other}"),
    }
}

/// The fields a limit error renders from — uniform across steps and memory.
struct LimitReport {
    code: &'static str,
    title: &'static str,
    /// The ceiling, already formatted for humans (`"2,000,000 steps"`, `"64.0 MiB"`).
    ceiling: String,
    /// Actual usage at the trip point, when it is worth reporting separately from
    /// the ceiling (memory). `None` for steps, where usage equals the limit.
    used: Option<Usage>,
    why: &'static str,
    flag: &'static str,
    json_key: &'static str,
    json_value: String,
}

/// Measured usage at the trip point, pre-formatted for both renderers.
struct Usage {
    human: String,
    bytes: u64,
}

fn render_limit(r: LimitReport, json: bool) -> String {
    if json {
        let used = match &r.used {
            Some(u) => format!(r#","used":{}"#, u.bytes),
            None => String::new(),
        };
        return format!(
            r#"{{"error":"limit-exceeded","code":"{}","{}":{}{}}}"#,
            r.code, r.json_key, r.json_value, used
        );
    }
    let halted = match &r.used {
        Some(u) => format!("used {} of the {} limit", u.human, r.ceiling),
        None => format!("the limit is {}", r.ceiling),
    };
    format!(
        "error: {} [{}]\n  \
         halted before completing — {}\n  \
         {}\n  \
         raise it with {flag} <VALUE>, or {flag} unbounded to opt out",
        r.title,
        r.code,
        halted,
        r.why,
        flag = r.flag,
    )
}

/// Group an integer with thousands separators: `2000000` -> `"2,000,000"`.
fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Human-readable binary size: `67108864` -> `"64.0 MiB"`.
fn human_size(bytes: u64) -> String {
    const KIB: u64 = 1 << 10;
    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} bytes")
    }
}

/// Minimal JSON string quoting for the rare `Display`-fallback path.
fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
