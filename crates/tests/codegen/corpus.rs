use std::fs;
use std::path::{Path, PathBuf};

use plotnik_compiler::InputKind;
pub(crate) use plotnik_tests::snapshot::Language as SourceLanguage;
use plotnik_tests::snapshot::{Fixture, Section};

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
    pub(crate) fixture: Fixture,
    pub(crate) language: SourceLanguage,
    pub(crate) grammar_json: PathBuf,
    pub(crate) expected: Expected,
}

pub(crate) struct Corpus {
    pub(crate) selected: usize,
    pub(crate) skipped: usize,
    pub(crate) inapplicable: usize,
    pub(crate) cases: Vec<Case>,
}

pub(crate) fn discover(manifest_dir: &Path, filter: Option<&str>) -> Result<Corpus, String> {
    let root = manifest_dir.join("tests/06-vm");
    let mut paths = Vec::new();
    discover_files(&root, &mut paths)?;
    paths.sort();

    let mut selected = 0;
    let mut skipped = 0;
    let mut inapplicable = 0;
    let mut cases = Vec::new();

    for path in paths {
        let relative = relative_snapshot_path(&root, &path)?;
        if filter.is_some_and(|needle| !relative.contains(needle)) {
            continue;
        }
        selected += 1;
        let raw = fs::read_to_string(&path)
            .map_err(|error| format!("read snapshot {}: {error}", path.display()))?;
        let fixture = Fixture::parse(&raw)
            .map_err(|error| format!("parse snapshot `{relative}`: {error}"))?;
        if fixture.kind == Some(InputKind::Expression) {
            inapplicable += 1;
            continue;
        }
        match case_from_fixture(&relative, fixture)? {
            Some(case) => cases.push(case),
            None => skipped += 1,
        }
    }

    Ok(Corpus {
        selected,
        skipped,
        inapplicable,
        cases,
    })
}

fn case_from_fixture(relative: &str, fixture: Fixture) -> Result<Option<Case>, String> {
    fixture.program().ok_or_else(|| {
        format!("snapshot `{relative}` has no `INPUT` section and cannot run natively")
    })?;
    let language = fixture.language();
    let output = unique_section(&fixture.generated, "output", relative)?;
    let diagnostics = unique_section(&fixture.generated, "diagnostics", relative)?;
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
        fixture,
        language,
        grammar_json: PathBuf::from(match language {
            SourceLanguage::JavaScript => env!("PLOTNIK_TEST_GRAMMAR_JAVASCRIPT"),
            SourceLanguage::TypeScript => env!("PLOTNIK_TEST_GRAMMAR_TYPESCRIPT"),
            SourceLanguage::Dart => env!("PLOTNIK_TEST_GRAMMAR_DART"),
        }),
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
