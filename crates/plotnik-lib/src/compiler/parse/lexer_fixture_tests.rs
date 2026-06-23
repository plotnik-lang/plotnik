use std::fs;
use std::path::{Path, PathBuf};

use similar::TextDiff;

use super::lexer::dump_tokens;

const FIXTURE_EXT: &str = "txt";

#[test]
fn lexer_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/01-lexer");
    let mut fixtures = Vec::new();
    discover(&root, &mut fixtures);
    fixtures.sort();
    assert!(
        !fixtures.is_empty(),
        "01-lexer fixtures should be present under {}",
        root.display()
    );

    let mut failures = Vec::new();
    for path in fixtures {
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
        "lexer fixtures out of date - run `make shot`:\n\n{}",
        failures.join("\n\n")
    );
}

fn discover(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("read fixture dir {}: {e}", dir.display()));
    for entry in entries {
        let entry =
            entry.unwrap_or_else(|e| panic!("read fixture entry in {}: {e}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            discover(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(FIXTURE_EXT) {
            out.push(path);
        }
    }
}

fn parse_query<'a>(raw: &'a str, path: &Path) -> &'a str {
    raw.split_once("\n==== tokens ====\n")
        .unwrap_or_else(|| {
            panic!(
                "fixture {} must contain an exact `==== tokens ====` section",
                path.display()
            )
        })
        .0
}

fn canonical(query: &str, tokens: &str) -> String {
    let mut out = String::new();
    out.push_str(query);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("==== tokens ====\n");
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
