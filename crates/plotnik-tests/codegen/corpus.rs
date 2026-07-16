use std::fs;
use std::path::{Path, PathBuf};

use plotnik_tests::snapshot::{Document, Section, parse_document};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceLanguage {
    JavaScript,
    TypeScript,
    Dart,
}

impl SourceLanguage {
    fn grammar_json(self) -> PathBuf {
        let path = match self {
            Self::JavaScript => env!("PLOTNIK_TEST_GRAMMAR_JAVASCRIPT"),
            Self::TypeScript => env!("PLOTNIK_TEST_GRAMMAR_TYPESCRIPT"),
            Self::Dart => env!("PLOTNIK_TEST_GRAMMAR_DART"),
        };
        PathBuf::from(path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Expected {
    NoMatch,
    Json(String),
}

impl Expected {
    pub(crate) fn text(&self) -> &str {
        match self {
            Self::NoMatch => "<no match>",
            Self::Json(json) => json,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Case {
    pub(crate) relative: String,
    pub(crate) query: String,
    pub(crate) source: String,
    pub(crate) language: SourceLanguage,
    pub(crate) grammar_json: PathBuf,
    pub(crate) expected: Expected,
}

pub(crate) struct Corpus {
    pub(crate) selected: usize,
    pub(crate) skipped: usize,
    pub(crate) cases: Vec<Case>,
}

pub(crate) fn discover(manifest_dir: &Path, filter: Option<&str>) -> Result<Corpus, String> {
    let root = manifest_dir.join("tests/06-vm");
    let mut paths = Vec::new();
    discover_files(&root, &mut paths)?;
    paths.sort();

    let mut selected = 0;
    let mut skipped = 0;
    let mut cases = Vec::new();

    for path in paths {
        let relative = relative_snapshot_path(&root, &path)?;
        if filter.is_some_and(|needle| !relative.contains(needle)) {
            continue;
        }
        selected += 1;
        let raw = fs::read_to_string(&path)
            .map_err(|error| format!("read snapshot {}: {error}", path.display()))?;
        let document = parse_document(&raw)
            .map_err(|error| format!("parse snapshot `{relative}`: {error}"))?;
        match case_from_document(&relative, document)? {
            Some(case) => cases.push(case),
            None => skipped += 1,
        }
    }

    Ok(Corpus {
        selected,
        skipped,
        cases,
    })
}

fn case_from_document(relative: &str, document: Document) -> Result<Option<Case>, String> {
    let input = document.sections.first().ok_or_else(|| {
        format!("snapshot `{relative}` has no `INPUT` section and cannot run natively")
    })?;
    let language = input_language(&input.name).ok_or_else(|| {
        format!("snapshot `{relative}` must begin with a supported `INPUT` section")
    })?;
    let output = unique_section(&document.sections, "output", relative)?;
    let diagnostics = unique_section(&document.sections, "diagnostics", relative)?;

    let Some(output) = output else {
        let Some(diagnostics) = diagnostics else {
            return Err(format!(
                "snapshot `{relative}` has neither `OUTPUT` nor `DIAGNOSTICS`"
            ));
        };
        if diagnostics.body.trim().is_empty() {
            return Err(format!(
                "snapshot `{relative}` has no `OUTPUT` and empty `DIAGNOSTICS`"
            ));
        }
        return Ok(None);
    };
    let expected = classify_expected(&output.body)
        .map_err(|error| format!("snapshot `{relative}` has invalid `OUTPUT`: {error}"))?;

    Ok(Some(Case {
        relative: relative.to_string(),
        query: document.query,
        source: input.body.clone(),
        language,
        grammar_json: language.grammar_json(),
        expected,
    }))
}

fn classify_expected(body: &str) -> Result<Expected, String> {
    if body == "<no match>" {
        return Ok(Expected::NoMatch);
    }
    serde_json::from_str::<serde_json::Value>(body)
        .map_err(|error| format!("expected JSON or `<no match>`: {error}"))?;
    Ok(Expected::Json(body.to_string()))
}

fn input_language(name: &str) -> Option<SourceLanguage> {
    match name {
        "input" | "input.js" | "input.javascript" | "input.jsx" => Some(SourceLanguage::JavaScript),
        "input.ts" | "input.typescript" => Some(SourceLanguage::TypeScript),
        "input.dart" => Some(SourceLanguage::Dart),
        _ => None,
    }
}

fn unique_section<'a>(
    sections: &'a [Section],
    name: &str,
    relative: &str,
) -> Result<Option<&'a Section>, String> {
    let mut matching = sections.iter().filter(|section| section.name == name);
    let found = matching.next();
    if matching.next().is_some() {
        return Err(format!(
            "snapshot `{relative}` contains more than one `{name}` section"
        ));
    }
    Ok(found)
}

fn relative_snapshot_path(root: &Path, path: &Path) -> Result<String, String> {
    path.strip_prefix(root)
        .map_err(|error| {
            format!(
                "snapshot path {} is outside corpus: {error}",
                path.display()
            )
        })?
        .to_str()
        .ok_or_else(|| format!("snapshot path is not UTF-8: {}", path.display()))
        .map(|relative| relative.replace(std::path::MAIN_SEPARATOR, "/"))
}

fn discover_files(directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("read corpus directory {}: {error}", directory.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("read entry under {}: {error}", directory.display()))?;
        let path = entry.path();
        if path.is_dir() {
            discover_files(&path, output)?;
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) == Some("txt") {
            output.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_match_and_json_null_are_distinct() {
        assert_eq!(classify_expected("<no match>").unwrap(), Expected::NoMatch);
        assert_eq!(
            classify_expected("null").unwrap(),
            Expected::Json("null".to_string())
        );
    }

    #[test]
    fn invalid_json_is_rejected_without_normalization() {
        let error = classify_expected("{ nope }").unwrap_err();

        assert!(error.contains("expected JSON"));
    }

    #[test]
    fn diagnostic_only_snapshot_is_not_runnable() {
        let document = parse_document(
            "Q = (program)\n--- INPUT ---\nx\n--- DIAGNOSTICS ---\ncompiler wording may change",
        )
        .unwrap();

        assert!(
            case_from_document("errors/example.txt", document)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn runnable_snapshot_without_output_or_diagnostics_is_rejected() {
        let document = parse_document("Q = (program)\n--- INPUT ---\nx").unwrap();

        let error = case_from_document("broken/example.txt", document).unwrap_err();

        assert!(error.contains("neither `OUTPUT` nor `DIAGNOSTICS`"));
    }

    #[test]
    fn filter_selects_before_returning_cases() {
        let corpus = discover(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            Some("captures/named_node.txt"),
        )
        .unwrap();

        assert_eq!(corpus.selected, 1);
        assert_eq!(corpus.skipped, 0);
        assert_eq!(corpus.cases.len(), 1);
        assert_eq!(corpus.cases[0].relative, "captures/named_node.txt");
    }

    #[test]
    fn input_sections_map_to_exact_grammar_artifacts() {
        let mappings = [
            ("input.js", SourceLanguage::JavaScript),
            ("input.ts", SourceLanguage::TypeScript),
            ("input.dart", SourceLanguage::Dart),
        ];

        for (section, expected) in mappings {
            let language = input_language(section).unwrap();
            assert_eq!(language, expected);
            assert!(
                language
                    .grammar_json()
                    .ends_with("grammar/src/grammar.json")
            );
        }
    }
}
