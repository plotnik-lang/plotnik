//! Compile-checks the golden `rust` sections of the 05-typegen corpus.
//!
//! The snapshot harness (`tests/mod.rs`) keeps the sections up to date; this
//! test proves the committed goldens are valid Rust against the real
//! `plotnik-rt`. Every section becomes one `mod` of a single generated
//! program, so name collisions across fixtures are impossible and the whole
//! corpus costs one `trybuild` compile. Extraction happens at test runtime
//! from the fixture files — a broken emitter fails this one test without
//! wedging `make shot`.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

#[test]
fn golden_rust_sections_compile() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/05-typegen");
    let mut mods = Vec::new();
    collect(&root, &root, &mut mods);
    assert!(!mods.is_empty(), "no RUST sections found under 05-typegen");
    mods.sort();

    let mut program = String::from("#![allow(dead_code)]\n");
    for (name, body) in &mods {
        write!(program, "\nmod {name} {{\n{body}}}\n").expect("writing to a String is infallible");
    }
    program.push_str("\nfn main() {}\n");

    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("golden-rust");
    fs::create_dir_all(&dir).expect("create trybuild scratch dir");
    let file = dir.join("all_typegen_fixtures.rs");
    fs::write(&file, program).expect("write generated program");

    let cases = trybuild::TestCases::new();
    cases.pass(&file);
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("read fixture dir {}: {e}", dir.display()));
    for entry in entries {
        let entry =
            entry.unwrap_or_else(|e| panic!("read fixture entry in {}: {e}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("txt") {
            continue;
        }
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()));
        // Diagnostics-only fixtures render no `rust` section; nothing to compile.
        let Some(body) = rust_section(&text) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .expect("fixture path is under the stage root")
            .with_extension("");
        out.push((mod_ident(&rel.to_string_lossy()), body));
    }
}

/// Body of the `RUST` section: the lines between its rule and the next rule.
fn rust_section(text: &str) -> Option<String> {
    let mut body = String::new();
    let mut in_rust = false;
    for line in text.lines() {
        match rule_label(line) {
            Some("RUST") => in_rust = true,
            Some(_) if in_rust => break,
            Some(_) => {}
            None if in_rust => {
                body.push_str(line);
                body.push('\n');
            }
            None => {}
        }
    }
    in_rust.then_some(body)
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

/// `serde/enum` → `fx_serde_enum`. The `fx_` prefix keeps keywords and
/// leading digits out of play.
fn mod_ident(rel: &str) -> String {
    let mut out = String::from("fx_");
    out.extend(
        rel.chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' }),
    );
    out
}
