//! Golden-fixture test suite.
//!
//! Each file under `tests/0N-stage/` is one fixture: an authored Plotnik query,
//! an optional `==== input ====` source section, and generated artifact sections
//! the harness rewrites in place on accept. The stage directory selects which
//! artifacts render:
//!
//! | dir          | sections                                   |
//! | ------------ | ------------------------------------------ |
//! | `02-parser`  | cst, ast                                   |
//! | `03-analyze` | symbols                                    |
//! | `04-emit`    | bytecode                                   |
//! | `05-typegen` | types                                      |
//! | `06-vm`      | bytecode, types, trace, output (requires input) |
//!
//! `==== diagnostics ====` renders whenever the query produces warnings or errors.
//! Errors are terminal for the compile stages (bytecode/types/trace/output are
//! suppressed); for `02-parser` they suppress cst/ast too, matching the recovery
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

use arborium_tree_sitter::{Language as TsLanguage, Parser as TsParser, Tree};
use libtest_mimic::{Arguments, Failed, Trial};
use similar::TextDiff;

use plotnik_lib::bytecode::{Module, dump as dump_bytecode};
use plotnik_lib::grammar::{Grammar, raw::RawGrammar};
use plotnik_lib::typegen::typescript;
use plotnik_lib::{
    Colors, PrintTracer, QueryBuilder, RuntimeError, SourceMap, VM, Verbosity, materialize_verified,
};

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
    let mut sections: Vec<(&str, Vec<&str>)> = Vec::new();
    let mut current: Option<(&str, Vec<&str>)> = None;

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
        return Err("fixture has no query (text before the first `==== … ====` section)".into());
    }

    // Input is authored only as the first section; an `input`-looking header
    // anywhere later belongs to a regenerated artifact, not to the source.
    let (input, generated_start) = match sections.first() {
        Some((name, body)) if *name == "input" || name.starts_with("input.") => {
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

fn validate_generated_headers(stage: &str, sections: &[(&str, Vec<&str>)]) -> Result<(), String> {
    let legal = generated_section_order(stage)
        .ok_or_else(|| format!("unknown stage directory `{stage}`"))?;
    let mut cursor = 0;

    for (name, _) in sections {
        let Some(offset) = legal[cursor..].iter().position(|known| known == name) else {
            return Err(format!(
                "section `==== {name} ====` is invalid or out of order for `{stage}`; exact fixture section headers are reserved in authored query/input text"
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
        "04" => Some(&["diagnostics", "bytecode"]),
        "05" => Some(&["diagnostics", "types"]),
        "06" => Some(&["diagnostics", "bytecode", "types", "trace", "output"]),
        _ => None,
    }
}

/// Only exact fixture section headers become boundaries. Stage-order validation
/// then rejects headers that look generated but appear in the wrong place.
fn parse_section_header(line: &str) -> Option<&str> {
    let inner = line.strip_prefix("==== ")?.strip_suffix(" ====")?;
    let known = inner == "input"
        || inner.starts_with("input.")
        || matches!(
            inner,
            "cst" | "ast" | "symbols" | "bytecode" | "types" | "trace" | "output" | "diagnostics"
        );
    known.then_some(inner)
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
        "04" => render_compile(query, input, Compile::Bytecode),
        "05" => render_compile(query, input, Compile::Types),
        "06" => render_compile(query, input, Compile::Vm),
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
    Types,
    Vm,
}

fn render_compile(
    query: &str,
    input: Option<&Input>,
    kind: Compile,
) -> Result<Vec<(String, String)>, String> {
    let lang = Lang::resolve(input.and_then(|i| i.ext.as_deref()))?;
    let linked = QueryBuilder::new(source_map(query))
        .link(lang.grammar)
        .expect("query parsing should not exhaust fuel");
    let diagnostics = linked.diagnostics();

    let mut out = Vec::new();
    if diagnostics.has_errors() {
        out.push((
            "diagnostics".into(),
            diagnostics.render(linked.source_map()),
        ));
        return Ok(out);
    }
    if diagnostics.has_warnings() {
        out.push((
            "diagnostics".into(),
            diagnostics.render(linked.source_map()),
        ));
    }

    let bytes = linked.emit().expect("valid query should emit");
    let module = Module::load(&bytes).expect("emitted bytecode should load");

    match kind {
        Compile::Bytecode => out.push((
            "bytecode".into(),
            dump_bytecode(&module, Colors::new(false)),
        )),
        Compile::Types => out.push(("types".into(), render_types(&module))),
        Compile::Vm => {
            out.push((
                "bytecode".into(),
                dump_bytecode(&module, Colors::new(false)),
            ));
            out.push(("types".into(), render_types(&module)));
            let input = input.ok_or_else(|| {
                "06-vm fixtures require an `==== input ====` section; compile-only fixtures belong in 04-emit/05-typegen".to_string()
            })?;
            let entry = linked
                .definition_names()
                .last()
                .expect("a valid query has at least one named definition");
            let (trace, output) = run_vm(&lang, &module, &entry, &input.text)?;
            out.push(("trace".into(), trace));
            out.push(("output".into(), output));
        }
    }
    Ok(out)
}

fn render_types(module: &Module) -> String {
    typescript::emit(
        module,
        typescript::Config::builder().emit_node_interface(false),
    )
}

fn run_vm(
    lang: &Lang,
    module: &Module,
    entry: &str,
    source: &str,
) -> Result<(String, String), String> {
    let tree = lang.parse(source);
    let entrypoint = module
        .entrypoint(entry)
        .expect("every named definition is an entrypoint");

    let vm = VM::builder(source, &tree).build();
    let mut tracer = PrintTracer::builder(source, module)
        .verbosity(Verbosity::Default)
        .colored(false)
        .build();

    let result = vm.execute_with(module, 0, &entrypoint, &mut tracer);
    let trace = tracer.render();
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
            value.format(false, Colors::new(false))
        }
        Err(RuntimeError::NoMatch) => "<no match>".to_string(),
        // A no-match is a real outcome worth pinning; fuel/recursion exhaustion is
        // not — fail the trial rather than accept a resource limit as golden output.
        Err(err) => return Err(format!("VM run failed for `{entry}`: {err}")),
    };
    Ok((trace, output))
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
            Some(other) => Err(format!(
                "input language `{other}` is not wired into the fixture suite yet (only JavaScript)"
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

fn javascript_grammar() -> &'static Grammar {
    static GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
        let raw = RawGrammar::from_json(include_str!(env!("PLOTNIK_LIB_JAVASCRIPT_GRAMMAR_JSON")))
            .expect("javascript grammar fixture");
        Grammar::from_raw(&raw).expect("javascript grammar metadata")
    });
    &GRAMMAR
}

fn source_map(query: &str) -> SourceMap {
    let mut sm = SourceMap::new();
    sm.add_file("query.ptk", query);
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
    out.push_str("==== ");
    out.push_str(name);
    out.push_str(" ====\n");
    out.push_str(body);
    if !out.ends_with('\n') {
        out.push('\n');
    }
}

fn push_section(out: &mut String, name: &str, body: &str) {
    out.push_str("==== ");
    out.push_str(name);
    out.push_str(" ====\n");
    let body = body.trim_matches('\n');
    if !body.is_empty() {
        out.push_str(body);
        out.push('\n');
    }
}

fn unified_diff(actual: &str, expected: &str) -> String {
    TextDiff::from_lines(actual, expected)
        .unified_diff()
        .context_radius(3)
        .header("on disk", "expected")
        .to_string()
}
