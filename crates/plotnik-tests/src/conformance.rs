//! Shared fixture discovery and trace rendering for executor conformance tests.

use std::fs;
use std::path::{Path, PathBuf};

use plotnik_rt::RuntimeEffect;

use crate::fixture::parse_section_header;

const DISCOVERY_FLOOR: usize = 250;

/// The authored portion of one `06-vm` golden fixture.
pub struct Fixture {
    /// Path relative to `tests/`, extension stripped: `06-vm/captures/...`.
    pub name: String,
    pub query: String,
    pub input: String,
    pub ext: Option<String>,
    /// VM-only channels which generated runtimes intentionally do not expose.
    pub excluded: Option<&'static str>,
}

/// Discover every `06-vm` fixture, including VM-only diagnostic fixtures.
pub fn collect_vm_fixtures() -> Result<Vec<Fixture>, String> {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let directory = tests.join("06-vm");
    let mut paths = Vec::new();
    collect_files(&directory, "txt", &mut paths)?;
    paths.sort();

    let mut fixtures = Vec::with_capacity(paths.len());
    for path in paths {
        let relative = path.strip_prefix(&tests).map_err(|error| {
            format!(
                "fixture {} is not below {}: {error}",
                path.display(),
                tests.display()
            )
        })?;
        let name = relative
            .with_extension("")
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let raw = fs::read_to_string(&path)
            .map_err(|error| format!("read fixture {}: {error}", path.display()))?;
        fixtures.push(parse_fixture(name, &raw)?);
    }

    if fixtures.len() < DISCOVERY_FLOOR {
        return Err(format!(
            "06-vm discovery collapsed: expected at least {DISCOVERY_FLOOR}, found {}",
            fixtures.len()
        ));
    }
    Ok(fixtures)
}

/// Stable compact rendering used by the Rust generated-matcher differential.
pub fn render_effect(effect: &RuntimeEffect<'_>) -> String {
    match effect {
        RuntimeEffect::Node(node) => format!(
            "Node {} {}..{}",
            node.kind_id(),
            node.start_byte(),
            node.end_byte()
        ),
        RuntimeEffect::ArrayOpen => "ArrayOpen".into(),
        RuntimeEffect::Push => "Push".into(),
        RuntimeEffect::ArrayClose => "ArrayClose".into(),
        RuntimeEffect::StructOpen => "StructOpen".into(),
        RuntimeEffect::Set(member) => format!("Set {member}"),
        RuntimeEffect::StructClose => "StructClose".into(),
        RuntimeEffect::EnumOpen(variant) => format!("EnumOpen {variant}"),
        RuntimeEffect::EnumClose => "EnumClose".into(),
        RuntimeEffect::Null => "Null".into(),
        RuntimeEffect::SpanStart { .. } | RuntimeEffect::SpanEnd(_) => {
            unreachable!("conformance queries compile without inspection")
        }
    }
}

fn collect_files(directory: &Path, extension: &str, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("read directory {}: {error}", directory.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("read entry in {}: {error}", directory.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, extension, out)?;
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) == Some(extension) {
            out.push(path);
        }
    }
    Ok(())
}

fn parse_fixture(name: String, raw: &str) -> Result<Fixture, String> {
    let normalized = raw.replace("\r\n", "\n");
    let mut query_lines = Vec::new();
    let mut input_lines = Vec::new();
    let mut ext = None;
    let mut zone = Zone::Query;

    for line in normalized.lines() {
        if let Some(section) = parse_section_header(line) {
            if matches!(zone, Zone::Query) {
                let Some(found) = section.strip_prefix("input") else {
                    return Err(format!(
                        "{name}: 06-vm fixture starts with `{section}`, expected INPUT"
                    ));
                };
                ext = found.strip_prefix('.').map(str::to_string);
                zone = Zone::Input;
                continue;
            }
            zone = Zone::Generated;
            continue;
        }

        match zone {
            Zone::Query => query_lines.push(line),
            Zone::Input => input_lines.push(line),
            Zone::Generated => {}
        }
    }

    if matches!(zone, Zone::Query) {
        return Err(format!("{name}: fixture has no INPUT section"));
    }

    let excluded = if name.contains("/inspection/") {
        Some("inspection effects are VM-only")
    } else if name.contains("/recording/") {
        Some("step recordings are VM-only")
    } else {
        None
    };
    Ok(Fixture {
        name,
        query: query_lines.join("\n"),
        input: input_lines.join("\n"),
        ext,
        excluded,
    })
}

enum Zone {
    Query,
    Input,
    Generated,
}

#[cfg(test)]
#[path = "conformance_tests.rs"]
mod conformance_tests;
