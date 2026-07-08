//! Golden-fixture test suite.
//!
//! Each file under `tests/0N-stage/` is one fixture: an authored Plotnik query,
//! an optional `INPUT` source section, and generated artifact sections
//! the harness rewrites in place on accept. The stage directory selects which
//! artifacts render.
//!
//! Sections are separated by a centered 50-column rule — `----- DIAGNOSTICS -----`.
//! The `INPUT` rule's parenthesized grammar selects what a query compiles against:
//! `INPUT (ts)` picks TypeScript, `INPUT (dart)` dart, plain `INPUT` JavaScript
//! (see `Lang::resolve`). Authors may dash the rule loosely (`--- INPUT (ts) ---`);
//! `make shot` normalizes it. For a 06-vm fixture the body is the source the VM
//! runs; for a compile-only stage (04-emit, 05-typegen) only the grammar matters,
//! so the body is left empty — the rule is a pure grammar selector. The stage
//! directory selects which artifacts render:
//!
//! | dir          | sections                                                                     |
//! | ------------ | ---------------------------------------------------------------------------- |
//! | `02-parser`  | cst, ast                                                                     |
//! | `03-analyze` | symbols                                                                      |
//! | `04-emit`    | nfa, bytecode                                                                |
//! | `05-typegen` | typescript, rust (serde impls under a `serde/` folder)                      |
//! | `06-vm`      | typescript, output, inspection if enabled, bytecode, trace (requires input) |
//!
//! Compile-stage fixtures under an `inspection/` folder compile with
//! `QueryBuilder::with_inspection(true)`.
//!
//! The `DIAGNOSTICS` section renders whenever the query produces warnings or errors.
//! Errors are terminal for the compile stages (bytecode/typescript/rust/trace/output
//! are suppressed); for `02-parser` they suppress cst/ast too, matching the recovery
//! tests. Warnings coexist with the normal sections.
//!
//! The `02-parser/trivia` folder renders its CST with trivia (whitespace/comments)
//! included — that attachment is exactly what those fixtures pin; every other
//! parser fixture omits trivia for a leaner tree.
//!
//! Run:   `cargo nextest run --test snapshots`
//! Accept: `SHOT=1 cargo nextest run --test snapshots`  (also wired into `make shot`)
//!
//! Trial names are the fixture path, so a `nextest` name filter scopes a run: a
//! stage (`06-vm`), a folder (`06-vm/captures`), or one construct across every
//! stage at once — `captures`, `quantifiers`, `anchors`, `alternations`,
//! `definitions`, `predicates`, `recursion`. That last form only stays complete
//! because every stage spells a construct identically; keep new folders on that
//! vocabulary.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use libtest_mimic::{Arguments, Failed, Trial};
use similar::TextDiff;
use tree_sitter::{Language as TsLanguage, Parser as TsParser, Tree};

use plotnik_lib::bytecode::{Module, dump as dump_bytecode};
use plotnik_lib::grammar::{Grammar, raw::RawGrammar};
use plotnik_lib::{
    Colors, CompiledQuery, DtsRange, MatcherConfig, PrintTracer, QueryBuilder, RecordingTracer,
    RuntimeError, RustConfig, SourceMap, SourcePath, TypeScriptConfig, VM, Verbosity,
    extract_inspection, materialize_verified,
};

mod support;

const FIXTURE_EXT: &str = "txt";

fn main() {
    let args = Arguments::from_args();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let trials = discover(&root)
        .into_iter()
        .map(|fx| {
            let name = fx.name.clone();
            Trial::test(name, move || run_fixture(&fx))
        })
        .collect();
    libtest_mimic::run(&args, trials).exit();
}

struct Fixture {
    path: PathBuf,
    name: String,
    stage: String,
}

fn discover(root: &Path) -> Vec<Fixture> {
    let mut out = Vec::new();
    // An unreadable tests root must fail loudly — silently yielding zero trials
    // would turn a broken checkout into a green run.
    let entries = fs::read_dir(root).expect("tests/ directory must be readable");
    for entry in entries {
        let entry =
            entry.unwrap_or_else(|e| panic!("read fixture entry in {}: {e}", root.display()));
        let path = entry.path();
        if path.is_dir()
            && let Some(stage) = path.file_name().and_then(|s| s.to_str())
            && is_stage_dir(stage)
        {
            let stage = stage.to_string();
            walk(&path, &stage, root, &mut out);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn is_stage_dir(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[2] == b'-'
        && !name.starts_with("01-")
}

fn walk(dir: &Path, stage: &str, root: &Path, out: &mut Vec<Fixture>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("read fixture dir {}: {e}", dir.display()));
    for entry in entries {
        let entry =
            entry.unwrap_or_else(|e| panic!("read fixture entry in {}: {e}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            walk(&path, stage, root, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(FIXTURE_EXT) {
            let rel = path
                .strip_prefix(root)
                .expect("fixture path is under tests root")
                .with_extension("");
            let name = rel
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            out.push(Fixture {
                path,
                name,
                stage: stage.to_string(),
            });
        }
    }
}

/// The authored half of a fixture; generated sections are recomputed, not parsed back.
struct Parsed {
    query: String,
    input: Option<Input>,
}

struct Input {
    ext: Option<String>,
    text: String,
}

fn run_fixture(fx: &Fixture) -> Result<(), Failed> {
    check(fx).map_err(Failed::from)
}

fn check(fx: &Fixture) -> Result<(), String> {
    let raw =
        fs::read_to_string(&fx.path).map_err(|e| format!("read {}: {e}", fx.path.display()))?;
    let parsed = parse_fixture(&raw, &fx.stage)?;
    let generated = render(&fx.stage, &fx.name, &parsed.query, parsed.input.as_ref())?;
    if generated.is_empty() {
        return Err(format!("stage `{}` produced no sections", fx.stage));
    }

    let expected = canonical(&parsed.query, parsed.input.as_ref(), &generated);
    let actual = raw.replace("\r\n", "\n");
    if actual == expected {
        return Ok(());
    }
    if shot_enabled() {
        fs::write(&fx.path, &expected).map_err(|e| format!("write {}: {e}", fx.path.display()))?;
        return Ok(());
    }
    Err(format!(
        "fixture out of date — run `make shot` (or `SHOT=1 cargo nextest run`):\n{}",
        unified_diff(&actual, &expected)
    ))
}

fn shot_enabled() -> bool {
    matches!(std::env::var("SHOT").as_deref(), Ok("1") | Ok("true"))
}

fn parse_fixture(raw: &str, stage: &str) -> Result<Parsed, String> {
    let normalized = raw.replace("\r\n", "\n");
    let mut query_lines: Vec<&str> = Vec::new();
    let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;

    for line in normalized.lines() {
        if let Some(name) = parse_section_header(line) {
            if let Some(prev) = current.take() {
                sections.push(prev);
            }
            current = Some((name, Vec::new()));
        } else if let Some((_, body)) = current.as_mut() {
            body.push(line);
        } else {
            query_lines.push(line);
        }
    }
    if let Some(prev) = current.take() {
        sections.push(prev);
    }

    let query = query_lines.join("\n");
    if query.trim().is_empty() {
        return Err("fixture has no query (text before the first `--- … ---` rule)".into());
    }

    // Input is authored only as the first section; an `input`-looking header
    // anywhere later belongs to a regenerated artifact, not to the source.
    let (input, generated_start) = match sections.first() {
        Some((name, body)) if name.as_str() == "input" || name.starts_with("input.") => {
            let ext = name
                .strip_prefix("input")
                .and_then(|rest| rest.strip_prefix('.'))
                .map(str::to_string);
            (
                Some(Input {
                    ext,
                    text: body.join("\n"),
                }),
                1,
            )
        }
        _ => (None, 0),
    };

    validate_generated_headers(stage, &sections[generated_start..])?;

    Ok(Parsed { query, input })
}

fn validate_generated_headers(stage: &str, sections: &[(String, Vec<&str>)]) -> Result<(), String> {
    let legal = generated_section_order(stage)
        .ok_or_else(|| format!("unknown stage directory `{stage}`"))?;
    let mut cursor = 0;

    for (name, _) in sections {
        let Some(offset) = legal[cursor..]
            .iter()
            .position(|known| *known == name.as_str())
        else {
            return Err(format!(
                "section `{name}` is invalid or out of order for `{stage}`; fixture section rules are reserved in authored query/input text"
            ));
        };
        cursor += offset + 1;
    }

    Ok(())
}

fn generated_section_order(stage: &str) -> Option<&'static [&'static str]> {
    match stage.split('-').next().unwrap_or("") {
        "02" => Some(&["diagnostics", "cst", "ast"]),
        "03" => Some(&["diagnostics", "symbols"]),
        "04" => Some(&["diagnostics", "nfa", "bytecode"]),
        "05" => Some(&["diagnostics", "typescript", "rust", "mapped"]),
        "07" => Some(&["diagnostics", "matcher"]),
        "06" => Some(&[
            "typescript",
            "diagnostics",
            "output",
            "inspection",
            "recording",
            "bytecode",
            "trace",
        ]),
        _ => None,
    }
}

/// Only fixture section rules become boundaries. Stage-order validation then
/// rejects rules that look generated but appear in the wrong place. A rule is a
/// label centered in dashes — `-------- DIAGNOSTICS --------`; the `INPUT` rule
/// carries its grammar in parens, `INPUT (ts)`.
fn parse_section_header(line: &str) -> Option<String> {
    let label = rule_label(line)?;
    let name = match label.split_once('(') {
        Some((head, ext)) if head.trim().eq_ignore_ascii_case("input") => {
            format!("input.{}", ext.strip_suffix(')')?.trim())
        }
        Some(_) => return None,
        None => label.to_ascii_lowercase(),
    };
    let known = name == "input"
        || name.starts_with("input.")
        || matches!(
            name.as_str(),
            "cst"
                | "ast"
                | "symbols"
                | "nfa"
                | "bytecode"
                | "mapped"
                | "typescript"
                | "rust"
                | "matcher"
                | "trace"
                | "output"
                | "inspection"
                | "recording"
                | "diagnostics"
        );
    known.then_some(name)
}

/// The label inside a `----- LABEL -----` rule, or `None` when the line isn't a
/// rule. A rule sits at column zero and pads its label with a space on each side
/// (` LABEL `), exactly as `section_rule` emits it; requiring that padding keeps
/// authored query/input bytes — a negated field `-types-`, an indented source
/// line — from being read as a section boundary.
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

fn render(
    stage: &str,
    name: &str,
    query: &str,
    input: Option<&Input>,
) -> Result<Vec<(String, String)>, String> {
    match stage.split('-').next().unwrap_or("") {
        // The `trivia` folder pins how whitespace/comments attach to the CST, so it
        // renders the trivia-inclusive CST; every other parser fixture omits trivia.
        "02" => Ok(render_frontend(
            query,
            Front::Parser {
                trivia: name.contains("/trivia/"),
            },
        )),
        "03" => Ok(render_frontend(query, Front::Analyze)),
        "04" => render_compile(name, query, input, Compile::Bytecode),
        "05" => render_compile(name, query, input, Compile::Typegen),
        "06" => render_compile(name, query, input, Compile::Vm),
        "07" => render_compile(name, query, input, Compile::Matcher),
        _ => Err(format!("unknown stage directory `{stage}`")),
    }
}

enum Front {
    Parser { trivia: bool },
    Analyze,
}

fn render_frontend(query: &str, kind: Front) -> Vec<(String, String)> {
    let analyzed = QueryBuilder::new(source_map(query))
        .analyze()
        .expect("query parsing should not exhaust fuel");
    let diagnostics = analyzed.diagnostics();
    let has_errors = diagnostics.has_errors();

    let mut out = Vec::new();
    if has_errors || diagnostics.has_warnings() {
        out.push((
            "diagnostics".into(),
            diagnostics.render(analyzed.source_map()),
        ));
    }
    match kind {
        // Parser recovery fixtures pin diagnostics only; a half-built error CST is noise.
        Front::Parser { trivia } if !has_errors => {
            let cst = analyzed.dump_cst_with_trivia(trivia);
            out.push(("cst".into(), cst));
            out.push(("ast".into(), analyzed.dump_ast()));
        }
        Front::Parser { .. } => {}
        // The symbol table is meaningful even with unresolved refs, so it renders
        // alongside any error diagnostics rather than being suppressed.
        Front::Analyze => {
            out.push(("symbols".into(), analyzed.dump_symbols()));
        }
    }
    out
}

enum Compile {
    Bytecode,
    Typegen,
    Vm,
    Matcher,
}

fn render_compile(
    name: &str,
    query: &str,
    input: Option<&Input>,
    kind: Compile,
) -> Result<Vec<(String, String)>, String> {
    let lang = Lang::resolve(input.and_then(|i| i.ext.as_deref()))?;
    let records = name.contains("/recording/");
    let inspects = name.contains("/inspection/");
    let strict_lints = name.contains("/lints/");
    let compiled = QueryBuilder::new(source_map(query))
        .with_inspection(inspects || records)
        .with_strict_lints(strict_lints)
        .compile(lang.grammar)
        .expect("query parsing should not exhaust fuel");
    let diagnostics = compiled.diagnostics();

    let mut out = Vec::new();
    if diagnostics.has_errors() {
        out.push((
            "diagnostics".into(),
            diagnostics.render(compiled.source_map()),
        ));
        return Ok(out);
    }
    let diag = diagnostics.has_warnings().then(|| {
        (
            "diagnostics".to_string(),
            diagnostics.render(compiled.source_map()),
        )
    });

    let module = compiled
        .module()
        .expect("valid query should compile to a module");

    match kind {
        Compile::Bytecode => {
            out.extend(diag);
            let nfa = compiled
                .dump_nfa(Colors::new(false))
                .expect("valid query should compile to a module");
            out.push(("nfa".into(), nfa));
            out.push(("bytecode".into(), dump_bytecode(module, Colors::new(false))));
        }
        Compile::Typegen => {
            out.extend(diag);
            let typescript = render_typescript(&compiled);
            out.push(("typescript".into(), typescript.clone()));
            let rust_config = RustConfig::new().serde(name.contains("/serde/"));
            let rust = compiled
                .to_rust(rust_config)
                .expect("valid query should compile to a module");
            out.push(("rust".into(), rust));
            if name.contains("/mapped/") {
                let (mapped_types, ranges) = compiled
                    .to_typescript_mapped(typegen_config())
                    .expect("valid query should compile to a module");
                assert_eq!(
                    typescript, mapped_types,
                    "mapped d.ts must match normal d.ts"
                );
                out.push(("mapped".into(), render_mapped(&mapped_types, &ranges)));
            }
        }
        Compile::Matcher => {
            out.extend(diag);
            let matcher = compiled
                .to_rust_matcher(MatcherConfig::new())
                .expect("valid query should compile to a module");
            out.push(("matcher".into(), matcher));
        }
        Compile::Vm => {
            let input = input.ok_or_else(|| {
                "06-vm fixtures require an `INPUT` section; compile-only fixtures belong in 04-emit/05-typegen".to_string()
            })?;
            let entry = module
                .entrypoint_names()
                .last()
                .ok_or_else(|| "06-vm fixture produced no callable entrypoints".to_string())?
                .to_string();
            let run = run_vm(&lang, module, &entry, &input.text, inspects, records)?;
            out.push(("typescript".into(), render_typescript(&compiled)));
            out.extend(diag);
            out.push(("output".into(), run.output));
            if let Some(inspection) = run.inspection {
                out.push(("inspection".into(), inspection));
            }
            if let Some(recording) = run.recording {
                out.push(("recording".into(), recording));
            }
            out.push(("bytecode".into(), dump_bytecode(module, Colors::new(false))));
            if let Some(trace) = run.trace {
                out.push(("trace".into(), trace));
            }
        }
    }
    Ok(out)
}

fn render_typescript(compiled: &CompiledQuery) -> String {
    compiled
        .to_typescript(typegen_config())
        .expect("valid query should compile to a module")
}

fn typegen_config() -> TypeScriptConfig {
    TypeScriptConfig::new().emit_node_interface(false)
}

fn render_mapped(dts: &str, ranges: &[DtsRange]) -> String {
    let mut out = String::new();
    for range in ranges {
        let start = range.start as usize;
        let end = range.end as usize;
        let member = range
            .member
            .map(|idx| format!(".M{idx}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "{}..{} T{}{} {:?}\n",
            range.start,
            range.end,
            range.type_id,
            member,
            &dts[start..end]
        ));
    }
    out
}

struct VmRun {
    trace: Option<String>,
    output: String,
    inspection: Option<String>,
    recording: Option<String>,
}

fn run_vm(
    lang: &Lang,
    module: &Module,
    entry: &str,
    source: &str,
    inspect: bool,
    record: bool,
) -> Result<VmRun, String> {
    let tree = lang.parse(source);
    let entrypoint = module
        .entrypoint(entry)
        .expect("selected definition must be an entrypoint");

    let vm = VM::builder(source, &tree).build();

    if record {
        let mut tracer = RecordingTracer::new(module, 65_536);
        let result = vm.execute_with(module, &entrypoint, &mut tracer);
        let recording = tracer.finish();
        let mut recording_json =
            serde_json::to_string_pretty(&recording).expect("recording serialization cannot fail");
        recording_json.push('\n');

        let output = match result {
            Ok(effects) => {
                // The verified variant (not plain `materialize`) so a type-unsound emission
                // panics the fixture in debug; the check compiles out under `--release`.
                let value = materialize_verified(
                    source,
                    module,
                    &entrypoint,
                    effects.as_slice(),
                    Colors::new(false),
                );
                value.format(true, Colors::new(false))
            }
            Err(RuntimeError::NoMatch) => "<no match>".to_string(),
            // A no-match is a real outcome worth pinning; step/memory exhaustion is
            // not — fail the trial rather than accept a resource limit as golden output.
            Err(err) => return Err(format!("VM run failed for `{entry}`: {err}")),
        };

        return Ok(VmRun {
            trace: None,
            output,
            inspection: None,
            recording: Some(recording_json),
        });
    }

    let mut tracer = PrintTracer::builder(source, module)
        .verbosity(Verbosity::Default)
        .colored(false)
        .build();

    let result = vm.execute_with(module, &entrypoint, &mut tracer);
    let trace = tracer.render();
    let (output, inspection) = match result {
        Ok(effects) => {
            let inspection = inspect.then(|| {
                let inspection = extract_inspection(effects.as_slice(), module);
                let mut rendered = serde_json::to_string_pretty(&inspection)
                    .expect("inspection serialization cannot fail");
                rendered.push('\n');
                rendered
            });
            // The verified variant (not plain `materialize`) so a type-unsound emission
            // panics the fixture in debug; the check compiles out under `--release`.
            let value = materialize_verified(
                source,
                module,
                &entrypoint,
                effects.as_slice(),
                Colors::new(false),
            );
            (value.format(true, Colors::new(false)), inspection)
        }
        Err(RuntimeError::NoMatch) => ("<no match>".to_string(), None),
        // A no-match is a real outcome worth pinning; step/memory exhaustion is
        // not — fail the trial rather than accept a resource limit as golden output.
        Err(err) => return Err(format!("VM run failed for `{entry}`: {err}")),
    };
    Ok(VmRun {
        trace: Some(trace),
        output,
        inspection,
        recording: None,
    })
}

struct Lang {
    grammar: &'static Grammar,
    ts: TsLanguage,
}

impl Lang {
    fn resolve(ext: Option<&str>) -> Result<Lang, String> {
        match ext {
            None | Some("js") | Some("javascript") | Some("jsx") => Ok(Lang {
                grammar: javascript_grammar(),
                ts: arborium_javascript::language().into(),
            }),
            Some("ts") | Some("typescript") => Ok(Lang {
                grammar: typescript_grammar(),
                ts: arborium_typescript::language().into(),
            }),
            Some("dart") => Ok(Lang {
                grammar: dart_grammar(),
                ts: arborium_dart::language().into(),
            }),
            Some(other) => Err(format!(
                "input language `{other}` is not wired into the fixture suite yet (have: javascript, typescript, dart)"
            )),
        }
    }

    fn parse(&self, source: &str) -> Tree {
        let mut parser = TsParser::new();
        parser
            .set_language(&self.ts)
            .expect("set tree-sitter language");
        parser.parse(source, None).expect("parse source")
    }
}

/// Define a lazily-loaded `&'static Grammar` from the `grammar.json` shipped by
/// the arborium dev-dependency. One per wired language.
macro_rules! grammar_loader {
    ($name:ident, $package:literal) => {
        fn $name() -> &'static Grammar {
            static GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
                let raw = RawGrammar::from_json(&support::load_arborium_grammar_json($package))
                    .expect(concat!($package, " grammar fixture"));
                Grammar::from_raw(&raw).expect(concat!($package, " grammar metadata"))
            });
            &GRAMMAR
        }
    };
}

grammar_loader!(javascript_grammar, "arborium-javascript");
grammar_loader!(typescript_grammar, "arborium-typescript");
grammar_loader!(dart_grammar, "arborium-dart");

fn source_map(query: &str) -> SourceMap {
    let mut sm = SourceMap::new();
    sm.add_file(SourcePath::new("query.ptk"), query);
    sm
}

/// The canonical file text every fixture is compared against. Rebuilding the whole
/// file each run, rather than editing sections in place, is what drops stale
/// sections on accept.
fn canonical(query: &str, input: Option<&Input>, generated: &[(String, String)]) -> String {
    let mut out = String::new();
    out.push_str(query);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if let Some(input) = input {
        let header = match &input.ext {
            Some(ext) => format!("input.{ext}"),
            None => "input".to_string(),
        };
        push_authored_section(&mut out, &header, &input.text);
    }
    for (name, body) in generated {
        push_section(&mut out, name, body);
    }
    out
}

fn push_authored_section(out: &mut String, name: &str, body: &str) {
    out.push_str(&section_rule(name));
    out.push('\n');
    out.push_str(body);
    if !out.ends_with('\n') {
        out.push('\n');
    }
}

fn push_section(out: &mut String, name: &str, body: &str) {
    out.push_str(&section_rule(name));
    out.push('\n');
    let body = body.trim_matches('\n');
    if !body.is_empty() {
        out.push_str(body);
        out.push('\n');
    }
}

/// A section boundary: the name centered in a 50-column dash rule. `input` and
/// `input.<ext>` render the grammar as `INPUT` / `INPUT (<ext>)`; every other
/// section is the uppercased name.
fn section_rule(name: &str) -> String {
    const WIDTH: usize = 50;
    let label = match name.strip_prefix("input.") {
        Some(ext) => format!("INPUT ({ext})"),
        None => name.to_ascii_uppercase(),
    };
    // At least one dash each side so an over-wide label still round-trips through
    // `rule_label`; width degrades gracefully past 50 columns.
    let fill = WIDTH.saturating_sub(label.len() + 2);
    let half = fill / 2;
    let left = half.max(1);
    let right = (fill - half).max(1);
    format!("{} {label} {}", "-".repeat(left), "-".repeat(right))
}

fn unified_diff(actual: &str, expected: &str) -> String {
    TextDiff::from_lines(actual, expected)
        .unified_diff()
        .context_radius(3)
        .header("on disk", "expected")
        .to_string()
}
