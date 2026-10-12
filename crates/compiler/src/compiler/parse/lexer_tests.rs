use std::fs;
use std::path::{Path, PathBuf};

use similar::TextDiff;

use super::lexer::dump_tokens;

const SNAPSHOT_EXT: &str = "txt";

#[test]
fn lexer_snapshots() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data/01-lexer");
    let mut snapshots = Vec::new();
    discover(&root, &mut snapshots);
    snapshots.sort();
    assert!(
        !snapshots.is_empty(),
        "01-lexer snapshots should be present under {}",
        root.display()
    );

    let mut failures = Vec::new();
    for path in snapshots {
        let raw =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let normalized = raw.replace("\r\n", "\n");
        let query = parse_query(&normalized, &path);
        let expected = canonical(query, &dump_tokens(query));

        if normalized == expected {
            continue;
        }

        if shot_enabled() {
            fs::write(&path, expected).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
            continue;
        }

        failures.push(format!(
            "{}\n{}",
            path.strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")))
                .unwrap_or(&path)
                .display(),
            unified_diff(&normalized, &expected)
        ));
    }

    assert!(
        failures.is_empty(),
        "lexer snapshots out of date - run `make shot`:\n\n{}",
        failures.join("\n\n")
    );
}

fn discover(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("read snapshot dir {}: {e}", dir.display()));
    for entry in entries {
        let entry =
            entry.unwrap_or_else(|e| panic!("read snapshot entry in {}: {e}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            discover(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(SNAPSHOT_EXT) {
            out.push(path);
        }
    }
}

fn parse_query<'a>(raw: &'a str, path: &Path) -> &'a str {
    // The query is everything before the `TOKENS` rule; the dump after it is
    // regenerated, so only the boundary matters.
    let mut offset = 0;
    for line in raw.split_inclusive('\n') {
        if is_tokens_rule(line) {
            return raw[..offset].strip_suffix('\n').unwrap_or(&raw[..offset]);
        }
        offset += line.len();
    }
    panic!(
        "snapshot {} must contain a `TOKENS` section rule",
        path.display()
    )
}

/// A column-zero, space-padded ` TOKENS ` rule — the shape `TOKENS_RULE` emits.
/// The padding keeps authored query bytes like `-tokens-` out of the boundary.
fn is_tokens_rule(line: &str) -> bool {
    let line = line.trim_end();
    line.starts_with('-')
        && line.ends_with('-')
        && line
            .trim_matches('-')
            .strip_prefix(' ')
            .and_then(|s| s.strip_suffix(' '))
            .is_some_and(|label| label.trim().eq_ignore_ascii_case("tokens"))
}

/// The rule that separates the authored query from the generated token dump —
/// `tokens` centered in a 50-column dash rule.
const TOKENS_RULE: &str = "--------------------- TOKENS ---------------------";

fn canonical(query: &str, tokens: &str) -> String {
    let mut out = String::new();
    out.push_str(query);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(TOKENS_RULE);
    out.push('\n');
    out.push_str(tokens.trim_matches('\n'));
    out.push('\n');
    out
}

fn shot_enabled() -> bool {
    matches!(std::env::var("SHOT").as_deref(), Ok("1") | Ok("true"))
}

fn unified_diff(actual: &str, expected: &str) -> String {
    TextDiff::from_lines(actual, expected)
        .unified_diff()
        .context_radius(3)
        .header("on disk", "expected")
        .to_string()
}
