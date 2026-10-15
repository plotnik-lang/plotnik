//! Snapshot test suite.
//!
//! Each file under `tests/0N-stage/` is one snapshot. Write a query at the top,
//! then add inputs as needed. The harness generates the remaining sections.
//! `make shot` updates them in place.
//!
//! Usually the opening query defines `Q`. It may instead be a bare expression.
//! VM tests run that expression when present, otherwise `Q`. A missing or
//! non-callable `Q` fails the test, not the query.
//!
//! `INPUT` contains JavaScript source. Use a filename to select another language:
//! `INPUT (index.ts)` for TypeScript or `INPUT (index.dart)` for Dart.
//! `INPUT (file.ptk)` adds query definitions to the same workspace. Use multiple
//! PTK inputs only when testing workspace composition. Each keeps its filename
//! and line numbers in diagnostics. Put all inputs before generated sections.
//!
//! Compiler-only snapshots can leave the programming input empty to select its
//! grammar, or replace it with `GRAMMAR`: a small YAML Tree-sitter grammar with
//! `name` omitted. `GRAMMAR` can accompany PTK inputs, but not programming input.
//!
//! Section rules are centered in 50 columns, such as `----- DIAGNOSTICS -----`.
//! Loose padding (`--- INPUT (index.ts) ---`) is accepted and normalized by
//! `make shot`. It also formats query modules, but preserves bare expressions.
//! The stage directory selects the generated sections:
//!
//! | dir          | sections                                                                     |
//! | ------------ | ---------------------------------------------------------------------------- |
//! | `02-parser`  | cst, ast                                                                     |
//! | `03-analyze` | definitions                                                                  |
//! | `04-emit/bytecode` | nfa, bytecode                                                         |
//! | `04-emit/types` | typescript, rust types (serde impls under a `serde/` folder)             |
//! | `04-emit/rust/module` | generated Rust matcher module                                      |
//! | `06-vm`      | typescript, output, inspection if enabled, bytecode, trace (requires input) |
//!
//! Compile-stage snapshots under an `inspection/` folder compile with
//! `BytecodeConfig::inspection(BytecodeInspection::Spans)`.
//!
//! The `DIAGNOSTICS` section renders whenever the query produces warnings or errors.
//! Errors are terminal for the compile stages (bytecode/typescript/rust/trace/output
//! are suppressed); for `02-parser` they suppress cst/ast too, matching the recovery
//! tests. Warnings coexist with the normal sections.
//!
//! The `02-parser/trivia` folder renders its CST with trivia (whitespace/comments)
//! included — that attachment is exactly what those snapshots pin; every other
//! parser snapshot omits trivia for a leaner tree.
//!
//! Run:   `cargo test -p plotnik-tests --test snapshots`
//! Accept: `SHOT=1 cargo test -p plotnik-tests --test snapshots` (also wired into `make shot`)
//!
//! Native test names are snake_case-normalized snapshot paths, so a name filter
//! scopes a run: a stage (`stage_06_vm`), a folder (`stage_06_vm::captures`), or
//! one construct across every stage at once — `captures`, `quantifiers`, `anchors`,
//! `alternations`, `definitions`, `predicates`, `recursion`. That last form only
//! stays complete because every stage spells a construct identically; keep new
//! folders on that vocabulary.
//!
//! Organize the corpus like a specification. Give each behavior a small snapshot of
//! its own before adding snapshots for interactions with other behaviors. Atomic
//! snapshots use the shortest unambiguous name in their construct folder; interaction
//! snapshots name the dimensions that make them more specific. A snapshot should not
//! contain unrelated behavior merely to exercise several features at once. Stage and
//! folder boundaries follow the current compiler pipeline and subsystem ownership,
//! not historical implementation phases.

mod grammar_limits;

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use similar::TextDiff;
use tree_sitter::{Language as TsLanguage, Parser as TsParser, Tree};

use plotnik_compiler::bytecode::{Entrypoint, Module, dump as dump_bytecode};
use plotnik_compiler::grammar::{Grammar, raw::RawGrammar};
use plotnik_compiler::{
    BytecodeConfig, BytecodeInspection, Colors, CompiledQuery, InputKind, PrintTracer,
    QueryBuilder, RuntimeError, RustCodegenConfig, SourceCollection, SourceMap, SourcePath,
    TraceRecorder, TypeScriptBinding, TypeScriptCodegenConfig, VM, Verbosity,
    extract_result_provenance, materialize_verified,
};
use plotnik_tests::snapshot::{Fixture, Language, Section};
use support::snapshots::{
    GeneratedOutput, GeneratedSection, InspectionPolicy, MappingPolicy, SectionKind, SerdePolicy,
    Snapshot, SnapshotKind, SnapshotMode, TriviaPolicy, VmMode, snapshot,
};

mod support;

fn run_snapshot(relative: &str) -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let snapshot = snapshot(&root, relative)?;
    let mode = snapshot_mode()?;
    check(&snapshot, mode)
}

fn snapshot_mode() -> Result<SnapshotMode, String> {
    static MODE: LazyLock<Result<SnapshotMode, String>> = LazyLock::new(SnapshotMode::from_env);
    match &*MODE {
        Ok(mode) => Ok(*mode),
        Err(error) => Err(error.clone()),
    }
}

fn formatted(parsed: &Fixture, kind: SnapshotKind, name: &str) -> Result<Fixture, String> {
    let mut formatted = parsed.clone();
    let opening = (formatted.kind == Some(InputKind::Module)).then_some(&mut formatted.query);
    for query in opening.into_iter().chain(
        formatted
            .inputs
            .iter_mut()
            .filter_map(|input| input.is_module().then_some(&mut input.body)),
    ) {
        let evaluated = support::formatter::evaluate(query, name)?;
        if !kind.preserves_query_layout() {
            *query = evaluated.into_query_or(query.clone());
        }
    }
    Ok(formatted)
}

fn check(snapshot: &Snapshot, mode: SnapshotMode) -> Result<(), String> {
    let raw = fs::read_to_string(&snapshot.path)
        .map_err(|e| format!("read {}: {e}", snapshot.path.display()))?;
    let parsed = parse_snapshot(&raw, snapshot.kind, snapshot.name.as_str())?;
    let actual = raw.replace("\r\n", "\n");
    let formatted = formatted(&parsed, snapshot.kind, snapshot.name.as_str())?;

    let accepting = matches!(mode, SnapshotMode::AcceptAll);
    let input = if accepting { &formatted } else { &parsed };
    let generated = render(snapshot.kind, input)?;
    if generated.is_empty() {
        return Err(format!(
            "snapshot `{}` produced no sections",
            snapshot.name.as_str()
        ));
    }
    let expected = canonical(input, &generated);
    if accepting {
        if actual != expected {
            support::atomic_file::replace(&snapshot.path, &expected)?;
        }
        return Ok(());
    }

    if actual != expected {
        return Err(format!(
            "snapshot out of date — run `make shot` (or `SHOT=1 cargo test -p plotnik-tests --test snapshots`):\n{}",
            unified_diff(&actual, &expected)
        ));
    }

    if parsed.query == formatted.query && parsed.inputs == formatted.inputs {
        return Ok(());
    }

    let generated = render(snapshot.kind, &formatted)?;
    Err(format!(
        "query formatting is out of date — run `make shot`:\n{}",
        unified_diff(&actual, &canonical(&formatted, &generated))
    ))
}

fn parse_snapshot(raw: &str, kind: SnapshotKind, name: &str) -> Result<Fixture, String> {
    let fixture = Fixture::parse(raw)?;
    if fixture.grammar().is_some()
        && matches!(
            kind,
            SnapshotKind::Parser { .. } | SnapshotKind::Analyze | SnapshotKind::Vm { .. }
        )
    {
        return Err("GRAMMAR is only supported in compiler snapshots".into());
    }
    if fixture.kind == Some(InputKind::Expression)
        && !matches!(
            kind,
            SnapshotKind::Vm { .. } | SnapshotKind::Bytecode { .. }
        )
    {
        return Err("expression fixtures require bytecode compilation or execution".into());
    }
    validate_generated_headers(kind, name, &fixture.generated)?;
    Ok(fixture)
}

fn validate_generated_headers(
    kind: SnapshotKind,
    name: &str,
    sections: &[plotnik_tests::snapshot::Section],
) -> Result<(), String> {
    let legal = kind.legal_sections();
    let mut cursor = 0;

    for section in sections {
        let header = &section.name;
        let Some(section_kind) = SectionKind::from_header(header) else {
            return Err(format!("unknown generated section `{header}` in `{name}`"));
        };
        let Some(offset) = legal[cursor..]
            .iter()
            .position(|known| *known == section_kind)
        else {
            return Err(format!(
                "section `{header}` is invalid or out of order for `{name}`; snapshot section rules are reserved in authored query/input text"
            ));
        };
        cursor += offset + 1;
    }

    Ok(())
}

fn render(kind: SnapshotKind, input: &Fixture) -> Result<Vec<GeneratedSection>, String> {
    let sections = match kind {
        SnapshotKind::Parser { trivia } => {
            render_frontend(input.collection().0, FrontendMode::Parser(trivia))?
        }
        SnapshotKind::Analyze => render_frontend(input.collection().0, FrontendMode::Analyze)?,
        compile => render_compile(compile, input)?,
    };
    Ok(GeneratedOutput::validate(kind, sections)?.into_sections())
}

enum FrontendMode {
    Parser(TriviaPolicy),
    Analyze,
}

fn render_frontend(
    sources: SourceCollection,
    kind: FrontendMode,
) -> Result<Vec<GeneratedSection>, String> {
    let analyzed = QueryBuilder::from_sources(sources)
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
        // Parser recovery snapshots pin diagnostics only; a half-built error CST is noise.
        FrontendMode::Parser(trivia) if !has_errors => {
            let cst = analyzed.dump_cst_with_trivia(matches!(trivia, TriviaPolicy::Include));
            out.push(GeneratedSection::new(SectionKind::Cst, cst));
            out.push(GeneratedSection::new(SectionKind::Ast, analyzed.dump_ast()));
        }
        FrontendMode::Parser(_) => {}
        // The definition tree is meaningful even with unresolved refs, so it renders
        // alongside any error diagnostics rather than being suppressed.
        FrontendMode::Analyze => {
            out.push(GeneratedSection::new(
                SectionKind::Definitions,
                analyzed.dump_definitions(),
            ));
        }
    }
    Ok(out)
}

fn render_compile(kind: SnapshotKind, parsed: &Fixture) -> Result<Vec<GeneratedSection>, String> {
    let lang = match parsed.grammar() {
        Some(yaml) => {
            let raw: RawGrammar = serde_saphyr::from_str(&format!("name: snapshot\n{yaml}"))
                .map_err(|error| format!("invalid GRAMMAR: {error}"))?;
            let grammar =
                Grammar::from_raw(&raw).map_err(|error| format!("invalid GRAMMAR: {error}"))?;
            Lang::Model(Box::new(grammar))
        }
        None => Lang::resolve(parsed.language()),
    };
    let (collection, expression) = parsed.collection();
    let builder = QueryBuilder::from_sources(collection).with_strict_lints(kind.strict_lints());
    let compiled = match expression {
        Some(expression) => builder
            .with_expression(expression)
            .compile_dynamic(lang.grammar()),
        None => builder.compile(lang.grammar()),
    }
    .map_err(|error| error.to_string())?;
    render_compiled(kind, &compiled, &lang, parsed.program())
}

fn render_compiled(
    kind: SnapshotKind,
    compiled: &CompiledQuery,
    lang: &Lang,
    input: Option<&Section>,
) -> Result<Vec<GeneratedSection>, String> {
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
        SnapshotKind::Bytecode { inspection, .. } => {
            let emission = emit_bytecode(compiled, inspection);
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
        SnapshotKind::Types { serde, mapping, .. } => {
            out.extend(diag);
            let typescript = render_typescript(compiled);
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
        SnapshotKind::Matcher { .. } => {
            out.extend(diag);
            let matcher = compiled
                .emit(RustCodegenConfig::new())
                .expect("Rust module emission answers")
                .into_artifact()
                .expect("valid query emits a Rust module")
                .into_source();
            out.push(GeneratedSection::new(SectionKind::Matcher, matcher));
        }
        SnapshotKind::Vm { mode, .. } => {
            let inspection = match mode {
                VmMode::StructuredTrace => InspectionPolicy::Include,
                VmMode::TextTrace { inspection } => inspection,
            };
            let emission = emit_bytecode(compiled, inspection);
            let module = emission
                .artifact()
                .expect("valid query should emit a bytecode module");
            let input = input.ok_or_else(|| {
                "06-vm snapshots require an `INPUT` section; compile-only snapshots belong in 04-emit".to_string()
            })?;
            let entry = module
                .expression_entrypoint()
                .or_else(|| module.entrypoint("Q"))
                .ok_or_else(|| {
                    "06-vm snapshots require an expression or a callable definition named `Q`"
                        .to_string()
                })?;
            let scenario = VmScenario {
                lang,
                module,
                entry: &entry,
                source: &input.body,
                mode,
            };
            if module.expression_entrypoint().is_none() {
                out.push(GeneratedSection::new(
                    SectionKind::TypeScript,
                    render_typescript(compiled),
                ));
            }
            out.extend(diag);
            if emission.diagnostics().has_warnings() {
                out.push(GeneratedSection::new(
                    SectionKind::Diagnostics,
                    emission.diagnostics().render(compiled.source_map()),
                ));
            }
            out.extend(render_vm(scenario)?);
        }
        SnapshotKind::Parser { .. } | SnapshotKind::Analyze => {
            unreachable!("frontend snapshots do not reach compilation")
        }
    }
    Ok(out)
}

fn render_vm(scenario: VmScenario<'_>) -> Result<Vec<GeneratedSection>, String> {
    let module = scenario.module;
    let mut out = Vec::new();
    match run_vm(scenario)? {
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
) -> plotnik_compiler::Emission<Module> {
    let config = if inspection == InspectionPolicy::Include {
        BytecodeConfig::new().inspection(BytecodeInspection::Spans)
    } else {
        BytecodeConfig::new()
    };
    let emission = compiled.emit(config).expect("bytecode emission answers");
    if let Some(module) = emission.artifact() {
        let analyzed = compiled.entrypoint_names().collect::<Vec<_>>();
        let emitted = module
            .entrypoint_names()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(
            emitted, analyzed,
            "emitted module entrypoints must preserve the analyzed entrypoint list",
        );
    }
    emission
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
    entry: &'a Entrypoint,
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
    let tree = scenario.lang.parse(scenario.source)?;
    let entrypoint = scenario.entry;

    let vm = VM::builder(scenario.source, &tree).build();

    if matches!(scenario.mode, VmMode::StructuredTrace) {
        let mut tracer = TraceRecorder::new(scenario.module, 65_536);
        let result = vm.execute_with(scenario.module, entrypoint, &mut tracer);
        let execution_trace = tracer.finish();
        let mut execution_trace_json = serde_json::to_string_pretty(&execution_trace)
            .expect("execution-trace serialization cannot fail");
        execution_trace_json.push('\n');

        let output = match result {
            Ok(journal) => {
                // The verified variant (not plain `materialize`) so a type-unsound emission
                // panics the snapshot in debug; the check compiles out under `--release`.
                let value = materialize_verified(
                    scenario.source,
                    scenario.module,
                    entrypoint,
                    journal.output_events(),
                    Colors::new(false),
                );
                plotnik_runtime::debug::to_json(&value)
                    .expect("materialized result must serialize to debug JSON")
            }
            Err(RuntimeError::NoMatch) => "<no match>".to_string(),
            // A no-match is a real outcome worth pinning; fuel/memory exhaustion is
            // not — fail the trial rather than accept a resource limit as expected output.
            Err(err) => {
                return Err(format!("VM run failed: {err}"));
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

    let result = vm.execute_with(scenario.module, entrypoint, &mut tracer);
    let trace = tracer.render();
    let (output, inspection) = match result {
        Ok(journal) => {
            let inspection = (inspection == InspectionPolicy::Include).then(|| {
                let result_provenance = extract_result_provenance(&journal, scenario.module);
                let mut rendered = serde_json::to_string_pretty(&result_provenance)
                    .expect("result provenance serialization cannot fail");
                rendered.push('\n');
                rendered
            });
            // The verified variant (not plain `materialize`) so a type-unsound emission
            // panics the snapshot in debug; the check compiles out under `--release`.
            let value = materialize_verified(
                scenario.source,
                scenario.module,
                entrypoint,
                journal.output_events(),
                Colors::new(false),
            );
            (
                plotnik_runtime::debug::to_json(&value)
                    .expect("materialized result must serialize to debug JSON"),
                inspection,
            )
        }
        Err(RuntimeError::NoMatch) => ("<no match>".to_string(), None),
        // A no-match is a real outcome worth pinning; fuel/memory exhaustion is
        // not — fail the trial rather than accept a resource limit as expected output.
        Err(err) => return Err(format!("VM run failed: {err}")),
    };
    Ok(VmArtifacts::TextTrace {
        output,
        trace,
        inspection,
    })
}

enum Lang {
    Source {
        grammar: &'static Grammar,
        ts: TsLanguage,
    },
    Model(Box<Grammar>),
}

impl Lang {
    fn resolve(language: Language) -> Lang {
        match language {
            Language::JavaScript => Self::Source {
                grammar: javascript_grammar(),
                ts: tree_sitter_javascript::LANGUAGE.into(),
            },
            Language::TypeScript => Self::Source {
                grammar: typescript_grammar(),
                ts: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            },
            Language::Dart => Self::Source {
                grammar: dart_grammar(),
                ts: tree_sitter_dart::LANGUAGE.into(),
            },
        }
    }

    fn grammar(&self) -> &Grammar {
        match self {
            Self::Source { grammar, .. } => grammar,
            Self::Model(grammar) => grammar,
        }
    }

    fn parse(&self, source: &str) -> Result<Tree, String> {
        let Self::Source { ts, .. } = self else {
            return Err("inline grammars support compiler checks only".into());
        };

        let mut parser = TsParser::new();
        parser.set_language(ts).expect("set tree-sitter language");
        Ok(parser.parse(source, None).expect("parse source"))
    }
}

macro_rules! grammar_loader {
    ($name:ident, $json:path, $description:literal) => {
        fn $name() -> &'static Grammar {
            static GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
                let raw = RawGrammar::from_json($json()).expect(concat!($description, " fixture"));
                Grammar::from_raw(&raw).expect(concat!($description, " metadata"))
            });
            &GRAMMAR
        }
    };
}

grammar_loader!(
    dart_grammar,
    support::grammar_loader::dart_grammar_json,
    "dart grammar"
);
grammar_loader!(
    javascript_grammar,
    support::javascript_grammar_json,
    "javascript grammar"
);
grammar_loader!(
    typescript_grammar,
    support::grammar_loader::typescript_grammar_json,
    "typescript grammar"
);

/// The canonical file text every snapshot is compared against. Rebuilding the whole
/// file each run, rather than editing sections in place, is what drops stale
/// sections on accept.
fn canonical(input: &Fixture, generated: &[GeneratedSection]) -> String {
    let mut out = String::new();
    out.push_str(&input.query);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    for section in &input.inputs {
        push_authored_section(&mut out, &section.name, &section.body);
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
/// `input.<filename>` render as `INPUT` / `INPUT (<filename>)`; every other
/// section is the uppercased name.
fn section_rule(name: &str) -> String {
    const WIDTH: usize = 50;
    let label = if let Some(ext) = name.strip_prefix("input.") {
        format!("INPUT ({ext})")
    } else {
        name.to_ascii_uppercase()
    };
    // At least one dash each side so an over-wide label still round-trips through
    // the snapshot parser; width degrades gracefully past 50 columns.
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

#[test]
fn one_module_runs_named_and_expression_entrypoints() {
    let lang = Lang::resolve(Language::JavaScript);
    let mut storage = SourceMap::new();
    storage.add_file(
        SourcePath::new("entries.ptk"),
        indoc::indoc! {"
        Part = { . (identifier) @text :: text . }
        A = (program (expression_statement (Part) @a))
        B = (program) @b :: text
        Expression = (number)
    "},
    );
    let expression = storage.add_stdin("(program (comment)? (expression_statement (Part) @expr))");
    let compiled = QueryBuilder::new(storage)
        .with_expression(expression)
        .compile_dynamic(lang.grammar())
        .unwrap();
    assert!(
        compiled.is_valid(),
        "{}",
        compiled.diagnostics().render(compiled.source_map())
    );
    assert_eq!(
        compiled.definition_names().collect::<Vec<_>>(),
        ["Part", "A", "B", "Expression"]
    );
    let module = compiled
        .emit(BytecodeConfig::new())
        .unwrap()
        .into_artifact()
        .unwrap();
    assert_eq!(
        module.entrypoint_names().collect::<Vec<_>>(),
        ["A", "B", "Expression"]
    );
    assert_eq!(module.entrypoint_count(), 4);
    assert!(module.entrypoint("Part").is_none());
    let a = module.entrypoint("A").unwrap();
    let b = module.entrypoint("B").unwrap();
    let expression = module.expression_entrypoint().unwrap();
    let source = "chosen;";
    let tree = lang.parse(source).unwrap();

    for (entry, expected) in [
        (a, serde_json::json!({"a": {"text": "chosen"}})),
        (b, serde_json::json!({"b": "chosen;"})),
        (expression, serde_json::json!({"expr": {"text": "chosen"}})),
        (a, serde_json::json!({"a": {"text": "chosen"}})),
    ] {
        let journal = VM::builder(source, &tree)
            .build()
            .execute(&module, &entry)
            .unwrap();
        let value = materialize_verified(
            source,
            &module,
            &entry,
            journal.output_events(),
            Colors::new(false),
        );
        let json = plotnik_runtime::debug::to_json(&value).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
            expected
        );
    }

    assert!(matches!(
        compiled.emit(RustCodegenConfig::new()),
        Err(plotnik_compiler::Error::EmitConfig(_))
    ));
    assert!(matches!(
        compiled.emit_types(TypeScriptCodegenConfig::new()),
        Err(plotnik_compiler::Error::EmitConfig(_))
    ));
}

#[test]
fn filtered_workspace_requires_query_input() {
    let mut storage = SourceMap::new();
    let foreign = storage.add_file(SourcePath::new("foreign.ptk"), "// @html\nInvalid = (");
    let expression = storage.add_stdin("(program)");
    let mut sources = SourceCollection::new(storage);
    sources.include_directory(foreign);

    for result in [
        QueryBuilder::from_sources(sources.clone()).compile(javascript_grammar()),
        QueryBuilder::from_sources(sources.clone()).compile_dynamic(javascript_grammar()),
    ] {
        let Err(error) = result else {
            panic!("a workspace with no selected query sources must fail compilation");
        };
        assert_eq!(
            error.to_string(),
            "nothing to compile: no query sources selected"
        );
    }

    let compiled = QueryBuilder::from_sources(sources)
        .with_expression(expression)
        .compile_dynamic(javascript_grammar())
        .unwrap();
    assert!(compiled.is_valid());
    let module = compiled
        .emit(BytecodeConfig::new())
        .unwrap()
        .into_artifact()
        .unwrap();
    assert_eq!(module.entrypoint_count(), 1);
    assert!(module.expression_entrypoint().is_some());
}

#[test]
fn static_generation_rechecks_dynamic_grouping_metadata() {
    let mut storage = SourceMap::new();
    let foreign = storage.add_file(SourcePath::new("foreign.ptk"), "// @html\nInvalid = (");
    let saved = storage.add_file(
        SourcePath::new("entries.ptk"),
        "// @bad/name\nRoot = (program)",
    );
    let mut sources = SourceCollection::new(storage);
    sources.include_directory(foreign);
    sources.include_directory(saved);
    let compiled = QueryBuilder::from_sources(sources)
        .compile_dynamic(javascript_grammar())
        .unwrap();
    assert!(compiled.is_valid());
    assert_eq!(compiled.definition_names().collect::<Vec<_>>(), ["Root"]);
    assert!(compiled.emit(BytecodeConfig::new()).unwrap().is_valid());

    let Err(plotnik_compiler::Error::QueryAnalyzeError(diagnostics)) =
        compiled.emit(RustCodegenConfig::new())
    else {
        panic!("static generation must reject malformed metadata admitted for dynamic execution");
    };
    let wire = diagnostics.to_wire(compiled.source_map());
    assert_eq!(wire.len(), 1);
    assert_eq!(
        wire[0].code,
        plotnik_compiler::DiagnosticKind::InvalidGrammarDeclaration
    );
    assert_eq!(wire[0].span.file, "entries.ptk");
    assert_eq!(
        (
            wire[0].span.start.line,
            wire[0].span.start.column,
            wire[0].span.start.offset
        ),
        (1, 5, 4)
    );
}

#[test]
fn expression_parse_errors_do_not_gain_semantic_errors() {
    let mut storage = SourceMap::new();
    let expression = storage.add_stdin("// no pattern");

    let compiled = QueryBuilder::new(storage)
        .with_expression(expression)
        .compile_dynamic(javascript_grammar())
        .unwrap();

    assert!(!compiled.is_valid());
    assert_eq!(compiled.diagnostics().error_count(), 1);
    let diagnostics = compiled.diagnostics().to_wire(compiled.source_map());
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].code,
        plotnik_compiler::DiagnosticKind::ExpectedExpression
    );
    assert_eq!(diagnostics[0].span.file, "<stdin>");
    assert_eq!(diagnostics[0].span.start.offset, 13);
    assert_eq!(diagnostics[0].span.end.offset, 13);
}

#[test]
fn entrypoint_advisories_include_the_execution_expression() {
    let mut storage = SourceMap::new();
    let saved = storage.add_file(SourcePath::new("entries.ptk"), "Named = (identifier)");
    let expression = storage.add_stdin("(identifier)");
    let mut sources = SourceCollection::new(storage);
    sources.include(saved);
    let compiled = QueryBuilder::from_sources(sources)
        .with_expression(expression)
        .with_strict_lints(true)
        .compile_dynamic(javascript_grammar())
        .unwrap();

    assert!(compiled.is_valid());
    let diagnostics = compiled.diagnostics().to_wire(compiled.source_map());
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(
        diagnostics[0].message,
        "entrypoint `Named` cannot match the `program` syntax-tree root"
    );
    assert_eq!(
        diagnostics[1].message,
        "this expression cannot match the `program` syntax-tree root"
    );
    for (diagnostic, (file, offset)) in diagnostics.iter().zip([("entries.ptk", 8), ("<stdin>", 0)])
    {
        assert_eq!(
            diagnostic.code,
            plotnik_compiler::DiagnosticKind::EntrypointNeverMatchesRoot
        );
        assert_eq!(diagnostic.span.file, file);
        assert_eq!(diagnostic.span.start.offset, offset);
        assert_eq!(diagnostic.hints, ["start with a `program` node pattern"]);
    }
}

include!(concat!(env!("OUT_DIR"), "/snapshot_tests.rs"));
