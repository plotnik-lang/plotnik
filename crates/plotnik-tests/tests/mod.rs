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
//! runs; for a compile-only emission fixture only the grammar matters,
//! so the body is left empty — the rule is a pure grammar selector. The stage
//! directory selects which artifacts render:
//!
//! | dir          | sections                                                                     |
//! | ------------ | ---------------------------------------------------------------------------- |
//! | `02-parser`  | cst, ast                                                                     |
//! | `03-analyze` | symbols                                                                      |
//! | `04-emit/bytecode` | nfa, bytecode                                                         |
//! | `04-emit/types` | typescript, rust types (serde impls under a `serde/` folder)             |
//! | `04-emit/rust/module` | generated Rust matcher module                                      |
//! | `06-vm`      | typescript, output, inspection if enabled, bytecode, trace (requires input) |
//!
//! Compile-stage fixtures under an `inspection/` folder compile with
//! `BytecodeConfig::inspection(BytecodeInspection::Spans)`.
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
//! Run:   `cargo test -p plotnik-tests --test snapshots`
//! Accept: `SHOT=1 cargo test -p plotnik-tests --test snapshots` (also wired into `make shot`)
//!
//! Native test names are snake_case-normalized fixture paths, so a name filter
//! scopes a run: a stage (`stage_06_vm`), a folder (`stage_06_vm::captures`), or
//! one construct across every stage at once — `captures`, `quantifiers`, `anchors`,
//! `alternations`, `definitions`, `predicates`, `recursion`. That last form only
//! stays complete because every stage spells a construct identically; keep new
//! folders on that vocabulary.
//!
//! Organize the corpus like a specification. Give each behavior a small fixture of
//! its own before adding fixtures for interactions with other behaviors. Atomic
//! fixtures use the shortest unambiguous name in their construct folder; interaction
//! fixtures name the dimensions that make them more specific. A fixture should not
//! contain unrelated behavior merely to exercise several features at once. Stage and
//! folder boundaries follow the current compiler pipeline and subsystem ownership,
//! not historical implementation phases.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use similar::TextDiff;
use tree_sitter::{Language as TsLanguage, Parser as TsParser, Tree};

use plotnik_lib::bytecode::{Module, dump as dump_bytecode};
use plotnik_lib::grammar::{Grammar, raw::RawGrammar};
use plotnik_lib::{
    BytecodeConfig, BytecodeInspection, Colors, CompiledQuery, PrintTracer, QueryBuilder,
    RuntimeError, RustCodegenConfig, SourceMap, SourcePath, TraceRecorder, TypeScriptBinding,
    TypeScriptCodegenConfig, VM, Verbosity, extract_result_provenance, materialize_verified,
};
use plotnik_tests::fixture::parse_section_header;
use support::formatter::Assessment;
use support::snapshots::{
    Fixture, FixtureKind, FixtureMode, GeneratedOutput, GeneratedSection, InspectionPolicy,
    MappingPolicy, SectionKind, SerdePolicy, TriviaPolicy, VmMode, fixture,
};

mod support;

fn run_fixture(relative: &str) -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let fx = fixture(&root, relative)?;
    let mode = fixture_mode()?;
    check(&fx, mode)
}

fn fixture_mode() -> Result<FixtureMode, String> {
    static MODE: LazyLock<Result<FixtureMode, String>> = LazyLock::new(FixtureMode::from_env);
    match &*MODE {
        Ok(mode) => Ok(*mode),
        Err(error) => Err(error.clone()),
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

fn check(fx: &Fixture, mode: FixtureMode) -> Result<(), String> {
    let raw =
        fs::read_to_string(&fx.path).map_err(|e| format!("read {}: {e}", fx.path.display()))?;
    let parsed = parse_fixture(&raw, fx.kind, fx.name.as_str())?;
    let actual = raw.replace("\r\n", "\n");
    let evaluated = support::formatter::evaluate(&parsed.query, fx.name.as_str())?;

    if matches!(mode, FixtureMode::AcceptAll) {
        let query = if fx.kind.preserves_query_layout() {
            parsed.query
        } else {
            evaluated.into_query_or(parsed.query)
        };
        let generated = render(fx.kind, &query, parsed.input.as_ref())?;
        if generated.is_empty() {
            return Err(format!(
                "fixture `{}` produced no sections",
                fx.name.as_str()
            ));
        }
        let accepted = canonical(&query, parsed.input.as_ref(), &generated);
        if actual != accepted {
            support::atomic_file::replace(&fx.path, &accepted)?;
        }
        return Ok(());
    }

    let generated = render(fx.kind, &parsed.query, parsed.input.as_ref())?;
    if generated.is_empty() {
        return Err(format!(
            "fixture `{}` produced no sections",
            fx.name.as_str()
        ));
    }
    let expected = canonical(&parsed.query, parsed.input.as_ref(), &generated);
    if actual != expected {
        return Err(format!(
            "fixture out of date — run `make shot` (or `SHOT=1 cargo test -p plotnik-tests --test snapshots`):\n{}",
            unified_diff(&actual, &expected)
        ));
    }

    if fx.kind.preserves_query_layout() {
        return Ok(());
    }
    let Assessment::Changed(query) = evaluated else {
        return Ok(());
    };

    let generated = render(fx.kind, &query, parsed.input.as_ref())?;
    let formatted = canonical(&query, parsed.input.as_ref(), &generated);
    Err(format!(
        "query formatting is out of date — run `make shot`:\n{}",
        unified_diff(&actual, &formatted)
    ))
}

fn parse_fixture(raw: &str, kind: FixtureKind, name: &str) -> Result<Parsed, String> {
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

    validate_generated_headers(kind, name, &sections[generated_start..])?;

    Ok(Parsed { query, input })
}

fn validate_generated_headers(
    kind: FixtureKind,
    name: &str,
    sections: &[(String, Vec<&str>)],
) -> Result<(), String> {
    let legal = kind.legal_sections();
    let mut cursor = 0;

    for (header, _) in sections {
        let Some(section_kind) = SectionKind::from_header(header) else {
            return Err(format!("unknown generated section `{header}` in `{name}`"));
        };
        let Some(offset) = legal[cursor..]
            .iter()
            .position(|known| *known == section_kind)
        else {
            return Err(format!(
                "section `{header}` is invalid or out of order for `{name}`; fixture section rules are reserved in authored query/input text"
            ));
        };
        cursor += offset + 1;
    }

    Ok(())
}

fn render(
    kind: FixtureKind,
    query: &str,
    input: Option<&Input>,
) -> Result<Vec<GeneratedSection>, String> {
    let sections = match kind {
        // The `trivia` folder pins how whitespace/comments attach to the CST, so it
        // renders the trivia-inclusive CST; every other parser fixture omits trivia.
        FixtureKind::Parser { trivia } => render_frontend(query, FrontendMode::Parser(trivia)),
        FixtureKind::Analyze => render_frontend(query, FrontendMode::Analyze),
        compile => render_compile(compile, query, input),
    }?;
    Ok(GeneratedOutput::validate(kind, sections)?.into_sections())
}

enum FrontendMode {
    Parser(TriviaPolicy),
    Analyze,
}

fn render_frontend(query: &str, kind: FrontendMode) -> Result<Vec<GeneratedSection>, String> {
    let analyzed = QueryBuilder::new(source_map(query))
        .analyze()
        .expect("query parsing should not exhaust fuel");
    let diagnostics = analyzed.diagnostics();
    let has_errors = diagnostics.has_errors();

    let mut out = Vec::new();
    if has_errors || diagnostics.has_warnings() {
        out.push(GeneratedSection::new(
            SectionKind::Diagnostics,
            diagnostics.render(analyzed.source_map()),
        ));
    }
    match kind {
        // Parser recovery fixtures pin diagnostics only; a half-built error CST is noise.
        FrontendMode::Parser(trivia) if !has_errors => {
            let cst = analyzed.dump_cst_with_trivia(matches!(trivia, TriviaPolicy::Include));
            out.push(GeneratedSection::new(SectionKind::Cst, cst));
            out.push(GeneratedSection::new(SectionKind::Ast, analyzed.dump_ast()));
        }
        FrontendMode::Parser(_) => {}
        // The symbol table is meaningful even with unresolved refs, so it renders
        // alongside any error diagnostics rather than being suppressed.
        FrontendMode::Analyze => {
            out.push(GeneratedSection::new(
                SectionKind::Symbols,
                analyzed.dump_symbols(),
            ));
        }
    }
    Ok(out)
}

fn render_compile(
    kind: FixtureKind,
    query: &str,
    input: Option<&Input>,
) -> Result<Vec<GeneratedSection>, String> {
    let lang = Lang::resolve(input.and_then(|i| i.ext.as_deref()))?;
    let compiled = QueryBuilder::new(source_map(query))
        .with_strict_lints(kind.strict_lints())
        .compile(lang.grammar)
        .expect("query parsing should not exhaust fuel");
    let diagnostics = compiled.diagnostics();

    let mut out = Vec::new();
    if diagnostics.has_errors() {
        out.push(GeneratedSection::new(
            SectionKind::Diagnostics,
            diagnostics.render(compiled.source_map()),
        ));
        return Ok(out);
    }
    let diag = diagnostics.has_warnings().then(|| {
        GeneratedSection::new(
            SectionKind::Diagnostics,
            diagnostics.render(compiled.source_map()),
        )
    });

    match kind {
        FixtureKind::Bytecode { inspection, .. } => {
            let emission = emit_bytecode(&compiled, inspection);
            let module = emission
                .artifact()
                .expect("valid query should emit a bytecode module");
            out.extend(diag);
            if emission.diagnostics().has_warnings() {
                out.push(GeneratedSection::new(
                    SectionKind::Diagnostics,
                    emission.diagnostics().render(compiled.source_map()),
                ));
            }
            let nfa = compiled
                .dump_nfa(Colors::new(false))
                .expect("valid query should compile to a module");
            out.push(GeneratedSection::new(SectionKind::Nfa, nfa));
            out.push(GeneratedSection::new(
                SectionKind::Bytecode,
                dump_bytecode(module, Colors::new(false)),
            ));
        }
        FixtureKind::Types { serde, mapping, .. } => {
            out.extend(diag);
            let typescript = render_typescript(&compiled);
            out.push(GeneratedSection::new(
                SectionKind::TypeScript,
                typescript.clone(),
            ));
            let rust_config = RustCodegenConfig::new().serde(matches!(serde, SerdePolicy::Include));
            let rust = compiled
                .emit_types(rust_config)
                .expect("Rust type emission answers")
                .into_artifact()
                .expect("valid query emits Rust types")
                .into_source();
            out.push(GeneratedSection::new(SectionKind::Rust, rust));
            if matches!(mapping, MappingPolicy::Include) {
                let (mapped_types, ranges) = compiled
                    .emit_types(typescript_config())
                    .expect("TypeScript emission answers")
                    .into_artifact()
                    .expect("valid query emits TypeScript types")
                    .into_parts();
                assert_eq!(
                    typescript, mapped_types,
                    "mapped d.ts must match normal d.ts"
                );
                out.push(GeneratedSection::new(
                    SectionKind::Mapped,
                    render_mapped(&mapped_types, &ranges),
                ));
            }
        }
        FixtureKind::Matcher { .. } => {
            out.extend(diag);
            let matcher = compiled
                .emit(RustCodegenConfig::new())
                .expect("Rust module emission answers")
                .into_artifact()
                .expect("valid query emits a Rust module")
                .into_source();
            out.push(GeneratedSection::new(SectionKind::Matcher, matcher));
        }
        FixtureKind::Vm { mode, .. } => {
            let inspection = match mode {
                VmMode::StructuredTrace => InspectionPolicy::Include,
                VmMode::TextTrace { inspection } => inspection,
            };
            let emission = emit_bytecode(&compiled, inspection);
            let module = emission
                .artifact()
                .expect("valid query should emit a bytecode module");
            let input = input.ok_or_else(|| {
                "06-vm fixtures require an `INPUT` section; compile-only fixtures belong in 04-emit".to_string()
            })?;
            let entry = module
                .entry_point_names()
                .last()
                .ok_or_else(|| "06-vm fixture produced no selectable entry points".to_string())?
                .to_string();
            let run = run_vm(VmScenario {
                lang: &lang,
                module,
                entry: &entry,
                source: &input.text,
                mode,
            })?;
            out.push(GeneratedSection::new(
                SectionKind::TypeScript,
                render_typescript(&compiled),
            ));
            out.extend(diag);
            if emission.diagnostics().has_warnings() {
                out.push(GeneratedSection::new(
                    SectionKind::Diagnostics,
                    emission.diagnostics().render(compiled.source_map()),
                ));
            }
            match run {
                VmArtifacts::StructuredTrace {
                    output,
                    execution_trace,
                } => {
                    out.push(GeneratedSection::new(SectionKind::Output, output));
                    out.push(GeneratedSection::new(
                        SectionKind::ExecutionTrace,
                        execution_trace,
                    ));
                }
                VmArtifacts::TextTrace {
                    output,
                    trace,
                    inspection,
                } => {
                    out.push(GeneratedSection::new(SectionKind::Output, output));
                    if let Some(inspection) = inspection {
                        out.push(GeneratedSection::new(SectionKind::Inspection, inspection));
                    }
                    out.push(GeneratedSection::new(
                        SectionKind::Bytecode,
                        dump_bytecode(module, Colors::new(false)),
                    ));
                    out.push(GeneratedSection::new(SectionKind::Trace, trace));
                    return Ok(out);
                }
            }
            out.push(GeneratedSection::new(
                SectionKind::Bytecode,
                dump_bytecode(module, Colors::new(false)),
            ));
        }
        FixtureKind::Parser { .. } | FixtureKind::Analyze => {
            unreachable!("frontend fixtures do not reach compilation")
        }
    }
    Ok(out)
}

fn render_typescript(compiled: &CompiledQuery) -> String {
    compiled
        .emit_types(typescript_config())
        .expect("TypeScript emission answers")
        .into_artifact()
        .expect("valid query emits TypeScript types")
        .into_parts()
        .0
}

fn typescript_config() -> TypeScriptCodegenConfig {
    TypeScriptCodegenConfig::new().emit_node_interface(false)
}

fn emit_bytecode(
    compiled: &CompiledQuery,
    inspection: InspectionPolicy,
) -> plotnik_lib::Emission<Module> {
    let config = if inspection == InspectionPolicy::Include {
        BytecodeConfig::new().inspection(BytecodeInspection::Spans)
    } else {
        BytecodeConfig::new()
    };
    compiled.emit(config).expect("bytecode emission answers")
}

fn render_mapped(dts: &str, bindings: &[TypeScriptBinding]) -> String {
    let mut out = String::new();
    for binding in bindings {
        let start = binding.span.0 as usize;
        let end = binding.span.1 as usize;
        let member = binding
            .member_id
            .map(|idx| format!(".M{idx}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "{}..{} T{}{} {:?}\n",
            binding.span.0,
            binding.span.1,
            binding.type_id,
            member,
            &dts[start..end]
        ));
    }
    out
}

struct VmScenario<'a> {
    lang: &'a Lang,
    module: &'a Module,
    entry: &'a str,
    source: &'a str,
    mode: VmMode,
}

enum VmArtifacts {
    StructuredTrace {
        output: String,
        execution_trace: String,
    },
    TextTrace {
        output: String,
        trace: String,
        inspection: Option<String>,
    },
}

fn run_vm(scenario: VmScenario<'_>) -> Result<VmArtifacts, String> {
    let tree = scenario.lang.parse(scenario.source);
    let entry_point = scenario
        .module
        .entry_point(scenario.entry)
        .expect("selected definition must be an entry point");

    let vm = VM::builder(scenario.source, &tree).build();

    if matches!(scenario.mode, VmMode::StructuredTrace) {
        let mut tracer = TraceRecorder::new(scenario.module, 65_536);
        let result = vm.execute_with(scenario.module, &entry_point, &mut tracer);
        let execution_trace = tracer.finish();
        let mut execution_trace_json = serde_json::to_string_pretty(&execution_trace)
            .expect("execution-trace serialization cannot fail");
        execution_trace_json.push('\n');

        let output = match result {
            Ok(effects) => {
                // The verified variant (not plain `materialize`) so a type-unsound emission
                // panics the fixture in debug; the check compiles out under `--release`.
                let value = materialize_verified(
                    scenario.source,
                    scenario.module,
                    &entry_point,
                    effects.as_slice(),
                    Colors::new(false),
                );
                value.format(true, Colors::new(false))
            }
            Err(RuntimeError::NoMatch) => "<no match>".to_string(),
            // A no-match is a real outcome worth pinning; fuel/memory exhaustion is
            // not — fail the trial rather than accept a resource limit as golden output.
            Err(err) => {
                return Err(format!("VM run failed for `{}`: {err}", scenario.entry));
            }
        };

        return Ok(VmArtifacts::StructuredTrace {
            output,
            execution_trace: execution_trace_json,
        });
    }

    let VmMode::TextTrace { inspection } = scenario.mode else {
        unreachable!("structured trace mode returns above")
    };
    let mut tracer = PrintTracer::builder(scenario.source, scenario.module)
        .verbosity(Verbosity::Default)
        .colored(false)
        .build();

    let result = vm.execute_with(scenario.module, &entry_point, &mut tracer);
    let trace = tracer.render();
    let (output, inspection) = match result {
        Ok(effects) => {
            let inspection = (inspection == InspectionPolicy::Include).then(|| {
                let result_provenance =
                    extract_result_provenance(effects.as_slice(), scenario.module);
                let mut rendered = serde_json::to_string_pretty(&result_provenance)
                    .expect("result provenance serialization cannot fail");
                rendered.push('\n');
                rendered
            });
            // The verified variant (not plain `materialize`) so a type-unsound emission
            // panics the fixture in debug; the check compiles out under `--release`.
            let value = materialize_verified(
                scenario.source,
                scenario.module,
                &entry_point,
                effects.as_slice(),
                Colors::new(false),
            );
            (value.format(true, Colors::new(false)), inspection)
        }
        Err(RuntimeError::NoMatch) => ("<no match>".to_string(), None),
        // A no-match is a real outcome worth pinning; step/memory exhaustion is
        // not — fail the trial rather than accept a resource limit as golden output.
        Err(err) => return Err(format!("VM run failed for `{}`: {err}", scenario.entry)),
    };
    Ok(VmArtifacts::TextTrace {
        output,
        trace,
        inspection,
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
                let raw = RawGrammar::from_json(support::load_arborium_grammar_json($package))
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
fn canonical(query: &str, input: Option<&Input>, generated: &[GeneratedSection]) -> String {
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
    for section in generated {
        push_section(&mut out, section.kind.as_str(), &section.body);
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
    // the fixture parser; width degrades gracefully past 50 columns.
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

include!(concat!(env!("OUT_DIR"), "/golden_tests.rs"));
