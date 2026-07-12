//! Adds the Rust type and module sections under `04-emit` to the generated
//! conformance program.
//!
//! The snapshot harness (`tests/mod.rs`) keeps the sections up to date; this
//! test proves the committed goldens are valid Rust against the real
//! `plotnik-rt`. Every section becomes one `mod`, so name collisions across
//! fixtures are impossible. The caller compiles them together with the 06-vm
//! generated matchers, keeping all generated-Rust validation in one rustc job.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

pub(super) fn append_golden_rust_sections(program: &mut String) {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut mods = Vec::new();
    collect(&tests, &tests.join("04-emit/types"), "RUST", &mut mods);
    collect(
        &tests,
        &tests.join("04-emit/rust/module"),
        "MATCHER",
        &mut mods,
    );
    assert!(!mods.is_empty(), "no Rust sections found in the corpus");
    mods.sort();

    for (name, body) in &mods {
        write!(program, "\nmod {name} {{\n{body}}}\n").expect("writing to a String is infallible");
    }
}

fn collect(root: &Path, dir: &Path, label: &str, out: &mut Vec<(String, String)>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("read fixture dir {}: {e}", dir.display()));
    for entry in entries {
        let entry =
            entry.unwrap_or_else(|e| panic!("read fixture entry in {}: {e}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, label, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("txt") {
            continue;
        }
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()));
        // Diagnostics-only fixtures render no Rust section; nothing to compile.
        let Some(body) = section(&text, label) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .expect("fixture path is under the tests root")
            .with_extension("");
        out.push((mod_ident(&rel.to_string_lossy()), body));
    }
}

/// Body of the `label` section: the lines between its rule and the next rule.
fn section(text: &str, label: &str) -> Option<String> {
    let mut body = String::new();
    let mut inside = false;
    for line in text.lines() {
        match rule_label(line) {
            Some(found) if found == label => inside = true,
            Some(_) if inside => break,
            Some(_) => {}
            None if inside => {
                body.push_str(line);
                body.push('\n');
            }
            None => {}
        }
    }
    inside.then_some(body)
}

/// Same rule shape `tests/mod.rs` emits: a label centered in dashes with one
/// space of padding each side. Rust code lines never match (a line would have
/// to both start and end with `-`).
fn rule_label(line: &str) -> Option<&str> {
    let line = line.trim_end();
    if !line.starts_with('-') || !line.ends_with('-') {
        return None;
    }
    let label = line
        .trim_matches('-')
        .strip_prefix(' ')?
        .strip_suffix(' ')?
        .trim();
    (!label.is_empty()).then_some(label)
}

/// `04-emit/types/serde/enum` → `fx_04_emit_types_serde_enum`. The `fx_` prefix
/// keeps keywords and leading digits out of play.
fn mod_ident(rel: &str) -> String {
    let mut out = String::from("fx_");
    out.extend(
        rel.chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' }),
    );
    out
}
