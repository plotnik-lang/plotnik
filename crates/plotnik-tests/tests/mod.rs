//! Snapshot test suite.
//!
//! Each file under `tests/0N-stage/` is one snapshot: an authored Plotnik query,
//! an optional `INPUT` or `GRAMMAR` section, and generated artifact sections
//! the harness rewrites in place on accept. The stage directory selects which
//! artifacts render.
//!
//! Sections are separated by a centered 50-column rule — `----- DIAGNOSTICS -----`.
//! The `INPUT` rule's parenthesized grammar selects what a query compiles against:
//! `INPUT (ts)` picks TypeScript, `INPUT (dart)` dart, plain `INPUT` JavaScript
//! (see `Lang::resolve`). Authors may dash the rule loosely (`--- INPUT (ts) ---`);
//! `GRAMMAR` supplies a small compiler-only grammar as YAML using the Tree-sitter
//! grammar schema, with `name` omitted. It replaces `INPUT`, never accompanies it.
//! `make shot` normalizes it. For a 06-vm snapshot the body is the source the VM
//! runs; for a compile-only emission snapshot only the grammar matters,
//! so the body is left empty — the rule is a pure grammar selector. The stage
//! directory selects which artifacts render:
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
//! Compiler snapshots may add `QUERY (file.ptk)` sections after the grammar/input
//! to compile additional query files alongside the initial `query.ptk`. An optional
//! YAML `LIMITS` section sets `work_budget` and optionally `automaton_max_depth`
//! through the compiler's public API.
//! `make shot` formats every query file and preserves the authored limit settings.
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
use plotnik_tests::snapshot::{Section, parse_document};
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

/// The authored half of a snapshot; generated sections are recomputed, not parsed back.
#[derive(Clone)]
struct Parsed {
    query: String,
    source: Option<Source>,
    files: Vec<Section>,
    limits: Option<String>,
}

impl Parsed {
    fn formatted(&self, kind: SnapshotKind, name: &str) -> Result<Self, String> {
        let mut formatted = self.clone();
        for query in std::iter::once(&mut formatted.query)
            .chain(formatted.files.iter_mut().map(|file| &mut file.body))
        {
            let evaluated = support::formatter::evaluate(query, name)?;
            if !kind.preserves_query_layout() {
                *query = evaluated.into_query_or(query.clone());
            }
        }

        Ok(formatted)
    }

    fn source_map(&self) -> SourceMap {
        let mut sources = SourceMap::new();
        sources.add_file(SourcePath::new("query.ptk"), &self.query);
        for file in &self.files {
            let path = file
                .name
                .strip_prefix("query.")
                .expect("validated query section");
            sources.add_file(SourcePath::new(path), &file.body);
        }
        sources
    }
}

#[derive(Clone)]
enum Source {
    Input(Input),
    Grammar(String),
}

impl Source {
    fn input(&self) -> Option<&Input> {
        match self {
            Self::Input(input) => Some(input),
            Self::Grammar(_) => None,
        }
    }
}

#[derive(Clone)]
struct Input {
    ext: Option<String>,
    text: String,
}

fn check(snapshot: &Snapshot, mode: SnapshotMode) -> Result<(), String> {
    let raw = fs::read_to_string(&snapshot.path)
        .map_err(|e| format!("read {}: {e}", snapshot.path.display()))?;
    let parsed = parse_snapshot(&raw, snapshot.kind, snapshot.name.as_str())?;
    let actual = raw.replace("\r\n", "\n");
    let formatted = parsed.formatted(snapshot.kind, snapshot.name.as_str())?;

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

    if parsed.query == formatted.query && parsed.files == formatted.files {
        return Ok(());
    }

    let generated = render(snapshot.kind, &formatted)?;
    Err(format!(
        "query formatting is out of date — run `make shot`:\n{}",
        unified_diff(&actual, &canonical(&formatted, &generated))
    ))
}

fn parse_snapshot(raw: &str, kind: SnapshotKind, name: &str) -> Result<Parsed, String> {
    let document = parse_document(raw)?;
    let sections = document.sections;

    let (source, generated_start) = match sections.first() {
        Some(section) if section.name == "grammar" => {
            if matches!(
                kind,
                SnapshotKind::Parser { .. } | SnapshotKind::Analyze | SnapshotKind::Vm { .. }
            ) {
                return Err("GRAMMAR is only supported in compiler snapshots".into());
            }
            (Some(Source::Grammar(section.body.clone())), 1)
        }
        Some(section) if section.name == "input" || section.name.starts_with("input.") => {
            let ext = section.name.strip_prefix("input.").map(str::to_string);
            (
                Some(Source::Input(Input {
                    ext,
                    text: section.body.clone(),
                })),
                1,
            )
        }
        _ => (None, 0),
    };

    let mut files = Vec::new();
    let mut limits = None;
    let mut generated_start = generated_start;
    while let Some(section) = sections.get(generated_start) {
        if !section.name.starts_with("query.") && section.name != "limits" {
            break;
        }
        if matches!(
            kind,
            SnapshotKind::Parser { .. } | SnapshotKind::Analyze | SnapshotKind::Vm { .. }
        ) {
            return Err(
                "QUERY and LIMITS sections are only supported in compiler snapshots".into(),
            );
        }
        if let Some(path) = section.name.strip_prefix("query.") {
            if path.is_empty()
                || path == "query.ptk"
                || files.iter().any(|file: &Section| file.name == section.name)
            {
                return Err(format!("duplicate or empty query path `{path}`"));
            }
            files.push(section.clone());
        } else if limits.replace(section.body.clone()).is_some() {
            return Err("duplicate LIMITS section".into());
        }
        generated_start += 1;
    }
    validate_generated_headers(kind, name, &sections[generated_start..])?;

    Ok(Parsed {
        query: document.query,
        source,
        files,
        limits,
    })
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

fn render(kind: SnapshotKind, input: &Parsed) -> Result<Vec<GeneratedSection>, String> {
    let sections = match kind {
        SnapshotKind::Parser { trivia } => {
            render_frontend(input.source_map(), FrontendMode::Parser(trivia))?
        }
        SnapshotKind::Analyze => render_frontend(input.source_map(), FrontendMode::Analyze)?,
        compile => render_compile(compile, input)?,
    };
    Ok(GeneratedOutput::validate(kind, sections)?.into_sections())
}

enum FrontendMode {
    Parser(TriviaPolicy),
    Analyze,
}

fn render_frontend(
    sources: SourceMap,
    kind: FrontendMode,
) -> Result<Vec<GeneratedSection>, String> {
    let analyzed = QueryBuilder::new(sources)
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

fn render_compile(kind: SnapshotKind, parsed: &Parsed) -> Result<Vec<GeneratedSection>, String> {
    let source = parsed.source.as_ref();
    let input = source.and_then(Source::input);
    let lang = match source {
        Some(Source::Grammar(yaml)) => {
            let raw: RawGrammar = serde_saphyr::from_str(&format!("name: snapshot\n{yaml}"))
                .map_err(|error| format!("invalid GRAMMAR: {error}"))?;
            let grammar =
                Grammar::from_raw(&raw).map_err(|error| format!("invalid GRAMMAR: {error}"))?;
            Lang::Model(Box::new(grammar))
        }
        _ => Lang::resolve(input.and_then(|input| input.ext.as_deref()))?,
    };
    let mut builder = QueryBuilder::new(parsed.source_map()).with_strict_lints(kind.strict_lints());
    if let Some(yaml) = &parsed.limits {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Limits {
            work_budget: u64,
            automaton_max_depth: Option<u32>,
        }

        let limits: Limits =
            serde_saphyr::from_str(yaml).map_err(|error| format!("invalid LIMITS: {error}"))?;
        builder = builder.with_satisfiability_work_budget(limits.work_budget);
        if let Some(depth) = limits.automaton_max_depth {
            builder = builder.with_satisfiability_automaton_max_depth(depth);
        }
    }
    let compiled = builder
        .compile(lang.grammar())
        .expect("query parsing should not exhaust fuel");
    render_compiled(kind, &compiled, &lang, input)
}

fn render_compiled(
    kind: SnapshotKind,
    compiled: &CompiledQuery,
    lang: &Lang,
    input: Option<&Input>,
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
            if module.entry_point("Q").is_none() {
                return Err("06-vm snapshots require a callable definition named `Q`".to_string());
            }
            let entry = "Q";
            let run = run_vm(VmScenario {
                lang,
                module,
                entry,
                source: &input.text,
                mode,
            })?;
            out.push(GeneratedSection::new(
                SectionKind::TypeScript,
                render_typescript(compiled),
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
        SnapshotKind::Parser { .. } | SnapshotKind::Analyze => {
            unreachable!("frontend snapshots do not reach compilation")
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
    let emission = compiled.emit(config).expect("bytecode emission answers");
    if let Some(module) = emission.artifact() {
        let analyzed = compiled.entry_point_names().collect::<Vec<_>>();
        let emitted = module
            .entry_point_names()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(
            emitted, analyzed,
            "emitted module entry points must preserve the analyzed entry-point list",
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
    let tree = scenario.lang.parse(scenario.source)?;
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
            Ok(journal) => {
                // The verified variant (not plain `materialize`) so a type-unsound emission
                // panics the snapshot in debug; the check compiles out under `--release`.
                let value = materialize_verified(
                    scenario.source,
                    scenario.module,
                    &entry_point,
                    journal.output_events(),
                    Colors::new(false),
                );
                plotnik_rt::debug::to_json(&value)
                    .expect("materialized result must serialize to debug JSON")
            }
            Err(RuntimeError::NoMatch) => "<no match>".to_string(),
            // A no-match is a real outcome worth pinning; fuel/memory exhaustion is
            // not — fail the trial rather than accept a resource limit as expected output.
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
                &entry_point,
                journal.output_events(),
                Colors::new(false),
            );
            (
                plotnik_rt::debug::to_json(&value)
                    .expect("materialized result must serialize to debug JSON"),
                inspection,
            )
        }
        Err(RuntimeError::NoMatch) => ("<no match>".to_string(), None),
        // A no-match is a real outcome worth pinning; fuel/memory exhaustion is
        // not — fail the trial rather than accept a resource limit as expected output.
        Err(err) => return Err(format!("VM run failed for `{}`: {err}", scenario.entry)),
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
    fn resolve(ext: Option<&str>) -> Result<Lang, String> {
        match ext {
            None | Some("js") | Some("javascript") | Some("jsx") => Ok(Self::Source {
                grammar: javascript_grammar(),
                ts: tree_sitter_javascript::LANGUAGE.into(),
            }),
            Some("ts") | Some("typescript") => Ok(Self::Source {
                grammar: typescript_grammar(),
                ts: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            }),
            Some("dart") => Ok(Self::Source {
                grammar: dart_grammar(),
                ts: tree_sitter_dart::LANGUAGE.into(),
            }),
            Some(other) => Err(format!(
                "source language `{other}` is not wired into the snapshot suite yet (have: javascript, typescript, dart)"
            )),
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
fn canonical(input: &Parsed, generated: &[GeneratedSection]) -> String {
    let source = input.source.as_ref();
    let mut out = String::new();
    out.push_str(&input.query);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if let Some(Source::Grammar(yaml)) = source {
        push_authored_section(&mut out, "grammar", yaml);
    }
    if let Some(input) = source.and_then(Source::input) {
        let header = match &input.ext {
            Some(ext) => format!("input.{ext}"),
            None => "input".to_string(),
        };
        push_authored_section(&mut out, &header, &input.text);
    }
    for file in &input.files {
        push_authored_section(&mut out, &file.name, &file.body);
    }
    if let Some(limits) = &input.limits {
        push_authored_section(&mut out, "limits", limits);
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
    let label = if let Some(ext) = name.strip_prefix("input.") {
        format!("INPUT ({ext})")
    } else if let Some(path) = name.strip_prefix("query.") {
        format!("QUERY ({path})")
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

include!(concat!(env!("OUT_DIR"), "/snapshot_tests.rs"));
