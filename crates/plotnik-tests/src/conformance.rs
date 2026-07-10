//! VM-oracle corpus shared by generated-runtime conformance runners.
//!
//! The exporter deliberately lives in the workspace-internal test crate: it
//! needs concrete grammar packages and the authored golden fixtures, neither
//! of which belongs in Plotnik's product CLI.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use plotnik_lib::grammar::{Grammar, raw::RawGrammar};
use plotnik_lib::{
    Colors, QueryBuilder, RuntimeError, SourceMap, SourcePath, VM, materialize_verified,
};
use plotnik_rt::RuntimeEffect;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tree_sitter::{Language as TsLanguage, Parser as TsParser};

pub const CORPUS_SCHEMA_VERSION: u32 = 1;
pub const RUNTIME_ABI: u32 = 1;

const DISCOVERY_FLOOR: usize = 250;
const RUNNABLE_FLOOR: usize = 200;

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

/// One effect in the language-neutral trace comparison format.
#[derive(Debug, Serialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum TraceEffect {
    Node {
        kind: String,
        kind_id: u16,
        text: String,
        span: [usize; 2],
    },
    ArrayOpen,
    Push,
    ArrayClose,
    StructOpen,
    Set {
        member: u16,
    },
    StructClose,
    EnumOpen {
        variant: u16,
    },
    EnumClose,
    Null,
}

/// Complete generated file set, keyed by path relative to the corpus root.
pub struct GeneratedCorpus {
    files: BTreeMap<PathBuf, String>,
    case_count: usize,
}

impl GeneratedCorpus {
    pub fn case_count(&self) -> usize {
        self.case_count
    }

    /// Verify that committed files exactly match a fresh VM-oracle export.
    pub fn check(&self, directory: &Path) -> Result<(), String> {
        let actual = collect_json_files(directory)?;
        let expected: BTreeSet<&Path> = self.files.keys().map(PathBuf::as_path).collect();
        let actual: BTreeSet<&Path> = actual.iter().map(PathBuf::as_path).collect();
        if actual != expected {
            let missing = expected
                .difference(&actual)
                .map(|p| p.display().to_string());
            let extra = actual
                .difference(&expected)
                .map(|p| p.display().to_string());
            return Err(format!(
                "corpus file set differs; missing [{}], extra [{}]",
                missing.collect::<Vec<_>>().join(", "),
                extra.collect::<Vec<_>>().join(", ")
            ));
        }

        for (relative, expected) in &self.files {
            let path = directory.join(relative);
            let actual = fs::read_to_string(&path)
                .map_err(|error| format!("read corpus file {}: {error}", path.display()))?;
            if actual != *expected {
                return Err(format!(
                    "{} is stale; run `cargo run -p plotnik-tests --bin export-conformance`",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    /// Write changed files and remove stale generated JSON cases.
    pub fn write(&self, directory: &Path) -> Result<(), String> {
        fs::create_dir_all(directory)
            .map_err(|error| format!("create corpus dir {}: {error}", directory.display()))?;

        let expected: BTreeSet<&Path> = self.files.keys().map(PathBuf::as_path).collect();
        for relative in collect_json_files(directory)? {
            if expected.contains(relative.as_path()) {
                continue;
            }
            let path = directory.join(&relative);
            fs::remove_file(&path)
                .map_err(|error| format!("remove stale corpus file {}: {error}", path.display()))?;
        }

        for (relative, contents) in &self.files {
            let path = directory.join(relative);
            let parent = path
                .parent()
                .expect("a corpus file path always has the corpus root as parent");
            fs::create_dir_all(parent)
                .map_err(|error| format!("create corpus dir {}: {error}", parent.display()))?;
            if fs::read_to_string(&path).is_ok_and(|old| old == *contents) {
                continue;
            }
            fs::write(&path, contents)
                .map_err(|error| format!("write corpus file {}: {error}", path.display()))?;
        }
        Ok(())
    }
}

/// The checked-in corpus directory at the workspace root.
pub fn default_corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/corpus")
}

/// Discover every `06-vm` fixture, including fixtures excluded from export.
pub fn collect_vm_fixtures() -> Result<Vec<Fixture>, String> {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let directory = tests.join("06-vm");
    let mut paths = Vec::new();
    collect_fixture_paths(&directory, &mut paths)?;
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

/// Generate deterministic cases from a fresh in-process VM run.
pub fn generate_corpus() -> Result<GeneratedCorpus, String> {
    let languages = LanguageSet::load()?;
    let fixtures = collect_vm_fixtures()?;
    let mut files = BTreeMap::new();
    let mut case_paths = Vec::new();
    let mut skipped = Vec::new();

    for fixture in fixtures {
        if let Some(reason) = fixture.excluded {
            skipped.push(SkippedFixture {
                fixture: fixture.name,
                reason: reason.to_string(),
            });
            continue;
        }

        let language = languages.resolve(fixture.ext.as_deref())?;
        match generate_case(&fixture, language)? {
            CaseResult::Case(case) => {
                let relative = case_path(&fixture.name);
                let contents = pretty_json(&case)?;
                case_paths.push(path_string(&relative));
                files.insert(relative, contents);
            }
            CaseResult::Skipped(reason) => skipped.push(SkippedFixture {
                fixture: fixture.name,
                reason,
            }),
        }
    }

    if case_paths.len() < RUNNABLE_FLOOR {
        return Err(format!(
            "runnable corpus collapsed: expected at least {RUNNABLE_FLOOR}, generated {}",
            case_paths.len()
        ));
    }

    let manifest = CorpusManifest {
        schema_version: CORPUS_SCHEMA_VERSION,
        runtime_abi: RUNTIME_ABI,
        case_count: case_paths.len(),
        cases: case_paths,
        skipped,
    };
    files.insert(PathBuf::from("manifest.json"), pretty_json(&manifest)?);
    Ok(GeneratedCorpus {
        case_count: manifest.case_count,
        files,
    })
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

fn trace_effect(effect: &RuntimeEffect<'_>, source: &str) -> Result<TraceEffect, String> {
    let trace = match effect {
        RuntimeEffect::Node(node) => {
            let span = [node.start_byte(), node.end_byte()];
            let text = source.get(span[0]..span[1]).ok_or_else(|| {
                format!(
                    "tree-sitter returned invalid UTF-8 source span {}..{}",
                    span[0], span[1]
                )
            })?;
            TraceEffect::Node {
                kind: node.kind().to_string(),
                kind_id: node.kind_id(),
                text: text.to_string(),
                span,
            }
        }
        RuntimeEffect::ArrayOpen => TraceEffect::ArrayOpen,
        RuntimeEffect::Push => TraceEffect::Push,
        RuntimeEffect::ArrayClose => TraceEffect::ArrayClose,
        RuntimeEffect::StructOpen => TraceEffect::StructOpen,
        RuntimeEffect::Set(member) => TraceEffect::Set { member: *member },
        RuntimeEffect::StructClose => TraceEffect::StructClose,
        RuntimeEffect::EnumOpen(variant) => TraceEffect::EnumOpen { variant: *variant },
        RuntimeEffect::EnumClose => TraceEffect::EnumClose,
        RuntimeEffect::Null => TraceEffect::Null,
        RuntimeEffect::SpanStart { .. } | RuntimeEffect::SpanEnd(_) => {
            return Err("inspection effects are not part of the runtime contract".to_string());
        }
    };
    Ok(trace)
}

fn generate_case(fixture: &Fixture, language: &LanguageData) -> Result<CaseResult, String> {
    let compiled = QueryBuilder::new(source_map(&fixture.query))
        .compile(&language.grammar)
        .map_err(|error| format!("{}: compile query: {error}", fixture.name))?;
    if compiled.diagnostics().has_errors() {
        return Ok(CaseResult::Skipped(
            "query intentionally has compile diagnostics".to_string(),
        ));
    }

    let Some(module) = compiled.module() else {
        return Err(format!(
            "{}: error-free query did not produce a bytecode module",
            fixture.name
        ));
    };
    let Some(entrypoint_name) = module.entrypoint_names().last() else {
        return Ok(CaseResult::Skipped(
            "query has no callable entrypoint".to_string(),
        ));
    };
    let entrypoint = module
        .entrypoint(entrypoint_name)
        .expect("a name from entrypoint_names resolves in the same module");

    let mut parser = TsParser::new();
    parser
        .set_language(&language.ts)
        .map_err(|error| format!("{}: set tree-sitter language: {error}", fixture.name))?;
    let tree = parser
        .parse(&fixture.input, None)
        .ok_or_else(|| format!("{}: tree-sitter parser returned no tree", fixture.name))?;
    let vm = VM::builder(&fixture.input, &tree).build();
    let result = vm.execute(module, &entrypoint);

    let (outcome, expected_trace, expected_value) = match result {
        Ok(effects) => {
            let trace = effects
                .as_slice()
                .iter()
                .map(|effect| trace_effect(effect, &fixture.input))
                .collect::<Result<Vec<_>, _>>()?;
            let value = materialize_verified(
                &fixture.input,
                module,
                &entrypoint,
                effects.as_slice(),
                Colors::OFF,
            );
            let value = serde_json::to_value(&value)
                .map_err(|error| format!("{}: serialize VM value: {error}", fixture.name))?;
            (Outcome::Match, Some(trace), Some(value))
        }
        Err(RuntimeError::NoMatch) => (Outcome::NoMatch, None, None),
        Err(error) => return Err(format!("{}: VM oracle failed: {error}", fixture.name)),
    };

    Ok(CaseResult::Case(Box::new(CorpusCase {
        schema_version: CORPUS_SCHEMA_VERSION,
        runtime_abi: RUNTIME_ABI,
        fixture: fixture.name.clone(),
        query: fixture.query.clone(),
        language: language.language,
        grammar: language.identity.clone(),
        source: fixture.input.clone(),
        entrypoint: entrypoint_name.to_string(),
        outcome,
        expected_trace,
        expected_value,
    })))
}

#[derive(Serialize)]
struct CorpusCase {
    schema_version: u32,
    runtime_abi: u32,
    fixture: String,
    query: String,
    language: &'static str,
    grammar: GrammarIdentity,
    source: String,
    entrypoint: String,
    outcome: Outcome,
    expected_trace: Option<Vec<TraceEffect>>,
    expected_value: Option<Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Match,
    NoMatch,
}

enum CaseResult {
    Case(Box<CorpusCase>),
    Skipped(String),
}

#[derive(Clone, Serialize)]
struct GrammarIdentity {
    name: String,
    sha256: String,
    source: String,
}

#[derive(Serialize)]
struct CorpusManifest {
    schema_version: u32,
    runtime_abi: u32,
    case_count: usize,
    cases: Vec<String>,
    skipped: Vec<SkippedFixture>,
}

#[derive(Serialize)]
struct SkippedFixture {
    fixture: String,
    reason: String,
}

struct LanguageSet {
    javascript: LanguageData,
    typescript: LanguageData,
    dart: LanguageData,
}

impl LanguageSet {
    fn load() -> Result<Self, String> {
        let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(&manifest_path)
            .exec()
            .map_err(|error| format!("resolve grammar packages from Cargo metadata: {error}"))?;

        Ok(Self {
            javascript: LanguageData::load(
                &metadata,
                "arborium-javascript",
                "javascript",
                arborium_javascript::language().into(),
            )?,
            typescript: LanguageData::load(
                &metadata,
                "arborium-typescript",
                "typescript",
                arborium_typescript::language().into(),
            )?,
            dart: LanguageData::load(
                &metadata,
                "arborium-dart",
                "dart",
                arborium_dart::language().into(),
            )?,
        })
    }

    fn resolve(&self, ext: Option<&str>) -> Result<&LanguageData, String> {
        match ext {
            None | Some("js" | "javascript" | "jsx") => Ok(&self.javascript),
            Some("ts" | "typescript") => Ok(&self.typescript),
            Some("dart") => Ok(&self.dart),
            Some(other) => Err(format!(
                "input language `{other}` is not wired into the conformance corpus"
            )),
        }
    }
}

struct LanguageData {
    language: &'static str,
    grammar: Grammar,
    ts: TsLanguage,
    identity: GrammarIdentity,
}

impl LanguageData {
    fn load(
        metadata: &cargo_metadata::Metadata,
        package_name: &str,
        language: &'static str,
        ts: TsLanguage,
    ) -> Result<Self, String> {
        let package = metadata
            .packages
            .iter()
            .find(|package| package.name.as_str() == package_name)
            .ok_or_else(|| format!("{package_name} package not found in Cargo metadata"))?;
        let root = package.manifest_path.parent().ok_or_else(|| {
            format!(
                "{package_name} manifest has no parent directory: {}",
                package.manifest_path
            )
        })?;
        let grammar_path = root.join("grammar/src/grammar.json");
        let bytes = fs::read(&grammar_path)
            .map_err(|error| format!("read {package_name} grammar at {grammar_path}: {error}"))?;
        let json = std::str::from_utf8(&bytes)
            .map_err(|error| format!("{package_name} grammar is not UTF-8: {error}"))?;
        let raw = RawGrammar::from_json(json)
            .map_err(|error| format!("parse {package_name} grammar: {error:?}"))?;
        let grammar = Grammar::from_raw(&raw)
            .map_err(|error| format!("lower {package_name} grammar metadata: {error:?}"))?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));

        Ok(Self {
            language,
            grammar,
            ts,
            identity: GrammarIdentity {
                name: raw.name,
                sha256,
                source: format!("{}@{}", package.name, package.version),
            },
        })
    }
}

fn collect_fixture_paths(directory: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("read fixture dir {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("read fixture entry in {}: {error}", directory.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_fixture_paths(&path, out)?;
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) == Some("txt") {
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
        if let Some(label) = rule_label(line) {
            if matches!(zone, Zone::Query) {
                let Some(found) = input_header_ext(label) else {
                    return Err(format!(
                        "{name}: 06-vm fixture starts with `{label}`, expected INPUT"
                    ));
                };
                ext = found;
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

/// `INPUT` → `Some(None)`, `INPUT (ts)` → `Some(Some("ts"))`, else `None`.
fn input_header_ext(label: &str) -> Option<Option<String>> {
    let rest = label.strip_prefix("INPUT")?.trim();
    if rest.is_empty() {
        return Some(None);
    }
    let ext = rest.strip_prefix('(')?.strip_suffix(')')?.trim();
    Some(Some(ext.to_string()))
}

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

fn source_map(query: &str) -> SourceMap {
    let mut source_map = SourceMap::new();
    source_map.add_file(SourcePath::new("query.ptk"), query);
    source_map
}

fn case_path(fixture_name: &str) -> PathBuf {
    PathBuf::from(
        fixture_name
            .strip_prefix("06-vm/")
            .expect("collected VM fixture names carry the 06-vm prefix"),
    )
    .with_extension("json")
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn pretty_json(value: &impl Serialize) -> Result<String, String> {
    let mut json = serde_json::to_string_pretty(value)
        .map_err(|error| format!("serialize conformance JSON: {error}"))?;
    json.push('\n');
    Ok(json)
}

fn collect_json_files(directory: &Path) -> Result<Vec<PathBuf>, String> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_json_files_below(directory, directory, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_json_files_below(
    directory: &Path,
    root: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("read corpus dir {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("read corpus entry in {}: {error}", directory.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_files_below(&path, root, out)?;
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let relative = path.strip_prefix(root).map_err(|error| {
            format!(
                "corpus file {} is not below {}: {error}",
                path.display(),
                root.display()
            )
        })?;
        out.push(relative.to_path_buf());
    }
    Ok(())
}
