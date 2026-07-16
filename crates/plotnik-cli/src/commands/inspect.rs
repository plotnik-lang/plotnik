//! One-shot compile/run bundle for playground and editor integrations.

use std::path::PathBuf;

use plotnik_lib::bytecode::{Labeling, Module, SPAN_NO_BINDING, SpanEntry, SpanKind};
use plotnik_lib::{
    BytecodeConfig, BytecodeInspection, Colors, NoopTracer, QueryBuilder, RuntimeError,
    RuntimeLimitSpec, TraceRecorder, TypeScriptCodegenConfig, VM, extract_result_provenance,
    materialize_verified, tokenize,
};
use serde_json::{Map, Value, json};

use super::query_loader::load_query;
use super::run_common;
use super::runtime_report::render_runtime_error;
use crate::error::{CliError, CliResult};

const DEFAULT_MAX_RECORDS: usize = 65_536;

macro_rules! json_value {
    ($value:expr) => {
        serde_json::to_value($value).expect("inspect bundle value serializes")
    };
}

pub struct InspectArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub entry: Option<String>,
    pub limits: RuntimeLimitSpec,
    pub json: bool,
    pub trace: bool,
    pub color: bool,
}

pub fn run(args: InspectArgs) -> CliResult {
    run_common::reject_ambiguous_inputs(
        args.query_text.as_deref(),
        args.query_path.as_deref(),
        args.source_text.as_deref(),
        args.source_path.as_deref(),
    )?;

    if args.source_path.is_none() && args.source_text.is_none() {
        return Err(CliError::fatal(
            "source is required: use positional argument or -s/--source",
        ));
    }

    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;
    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    let source = loaded
        .sources
        .iter()
        .next()
        .expect("non-empty query has a source");
    let query_tokens = tokenize(source.content);
    let declared_lang = loaded.shebang.lang.clone();
    let shebang_entry = loaded.shebang.entry.clone();

    let source_code = run_common::load_source(
        args.source_text.as_deref(),
        args.source_path.as_deref(),
        args.query_path.as_deref(),
    )?;
    let lang = run_common::resolve_run_lang(
        args.lang.as_deref(),
        declared_lang.as_deref(),
        args.source_path.as_deref(),
    )?;

    let compiled = QueryBuilder::new(loaded.sources)
        .compile(lang.grammar())
        .map_err(|e| CliError::fatal(e.to_string()))?;

    let types = compiled
        .emit_types(TypeScriptCodegenConfig::new().colored(false))
        .map_err(|error| CliError::fatal(error.to_string()))?;
    let type_diagnostics = types.diagnostics().clone();
    let (typescript_declarations, typescript_bindings) = types
        .into_artifact()
        .map(|output| output.into_parts())
        .unwrap_or_else(|| (String::new(), Vec::new()));
    let bytecode = compiled
        .emit(BytecodeConfig::new().inspection(BytecodeInspection::Spans))
        .map_err(|error| CliError::fatal(error.to_string()))?;
    let mut diagnostics = compiled.diagnostics().clone();
    diagnostics.extend(type_diagnostics);
    diagnostics.extend(bytecode.diagnostics().clone());
    let diagnostics_have_errors = diagnostics.has_errors();
    let diagnostics = diagnostics.to_wire(compiled.source_map());
    let module = bytecode.into_artifact();
    let query_spans = module
        .as_ref()
        .map(query_spans_json)
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let entry_points = module.as_ref().map(entry_point_names).unwrap_or_default();

    let run = if let Some(module) = module.as_ref() {
        let default_entry = module.entry_point_names().last().map(str::to_owned);
        let entry = args.entry.clone().or(shebang_entry).or(default_entry);
        let entry_point = run_common::resolve_entry_point(module, entry.as_deref())?;
        let tree = lang.parse_source(&source_code);
        run_module(
            module,
            &entry_point,
            &source_code,
            &tree,
            args.limits,
            args.trace,
        )
    } else {
        RunPayload::not_run()
    };

    let bundle = bundle_json(BundleParts {
        query_spans,
        query_tokens: json_value!(query_tokens),
        diagnostics: json_value!(diagnostics),
        typescript_declarations,
        typescript_bindings: json_value!(typescript_bindings),
        entry_points: json_value!(entry_points),
        run: &run,
    });

    if args.json {
        println!("{}", bundle);
    } else {
        print_summary(&bundle, args.color);
    }

    if diagnostics_have_errors {
        return Err(CliError::No);
    }
    match run.exit {
        InspectExit::Ok => Ok(()),
        InspectExit::NoMatch => Err(CliError::No),
        InspectExit::RuntimeError => Err(CliError::FatalRendered),
    }
}

struct BundleParts<'a> {
    query_spans: Value,
    query_tokens: Value,
    diagnostics: Value,
    typescript_declarations: String,
    typescript_bindings: Value,
    entry_points: Value,
    run: &'a RunPayload,
}

fn bundle_json(parts: BundleParts<'_>) -> Value {
    let mut object = Map::new();
    object.insert("query_spans".to_string(), parts.query_spans);
    object.insert("query_tokens".to_string(), parts.query_tokens);
    object.insert("diagnostics".to_string(), parts.diagnostics);
    object.insert(
        "typescript_declarations".to_string(),
        Value::String(parts.typescript_declarations),
    );
    object.insert("typescript_bindings".to_string(), parts.typescript_bindings);
    object.insert("entry_points".to_string(), parts.entry_points);
    object.insert("result".to_string(), parts.run.result.clone());
    object.insert(
        "result_provenance".to_string(),
        parts.run.result_provenance.clone(),
    );
    object.insert("run_stats".to_string(), parts.run.run_stats.clone());
    object.insert(
        "execution_trace".to_string(),
        parts.run.execution_trace.clone(),
    );
    if let Some(error) = &parts.run.error {
        object.insert("error".to_string(), error.clone());
    }
    Value::Object(object)
}

fn run_module(
    module: &Module,
    entry_point: &plotnik_lib::bytecode::EntryPoint,
    source_code: &str,
    tree: &tree_sitter::Tree,
    limits: RuntimeLimitSpec,
    trace: bool,
) -> RunPayload {
    let vm = VM::builder(source_code, tree).limits(limits).build();
    if trace {
        let mut tracer = TraceRecorder::new(module, DEFAULT_MAX_RECORDS);
        let (result, stats) = vm.execute_with_stats(module, entry_point, &mut tracer);
        let execution_trace = tracer.finish();
        return run_payload_from_result(
            module,
            entry_point,
            source_code,
            (result, stats),
            Some(json_value!(execution_trace)),
        );
    }

    let mut tracer = NoopTracer;
    let (result, stats) = vm.execute_with_stats(module, entry_point, &mut tracer);
    run_payload_from_result(module, entry_point, source_code, (result, stats), None)
}

fn run_payload_from_result(
    module: &Module,
    entry_point: &plotnik_lib::bytecode::EntryPoint,
    source_code: &str,
    result: (
        Result<plotnik_lib::MatchJournal<'_>, RuntimeError>,
        plotnik_lib::RunStats,
    ),
    execution_trace: Option<Value>,
) -> RunPayload {
    let (result, stats) = result;
    match result {
        Ok(journal) => {
            let colors = Colors::new(false);
            let result = materialize_verified(
                source_code,
                module,
                entry_point,
                journal.output_events(),
                colors,
            );
            let result_provenance =
                (!module.spans().is_empty()).then(|| extract_result_provenance(&journal, module));
            RunPayload {
                result: json_value!(result),
                result_provenance: json_value!(result_provenance),
                run_stats: json_value!(stats),
                execution_trace: execution_trace.unwrap_or(Value::Null),
                error: None,
                exit: InspectExit::Ok,
            }
        }
        Err(RuntimeError::NoMatch) => RunPayload {
            result: Value::Null,
            result_provenance: Value::Null,
            run_stats: Value::Null,
            execution_trace: execution_trace.unwrap_or(Value::Null),
            error: Some(Value::String("no match".to_string())),
            exit: InspectExit::NoMatch,
        },
        Err(error) => RunPayload {
            result: Value::Null,
            result_provenance: Value::Null,
            run_stats: Value::Null,
            execution_trace: execution_trace.unwrap_or(Value::Null),
            error: Some(runtime_error_value(&error)),
            exit: InspectExit::RuntimeError,
        },
    }
}

struct RunPayload {
    result: Value,
    result_provenance: Value,
    run_stats: Value,
    execution_trace: Value,
    error: Option<Value>,
    exit: InspectExit,
}

impl RunPayload {
    fn not_run() -> Self {
        Self {
            result: Value::Null,
            result_provenance: Value::Null,
            run_stats: Value::Null,
            execution_trace: Value::Null,
            error: None,
            exit: InspectExit::Ok,
        }
    }
}

#[derive(Clone, Copy)]
enum InspectExit {
    Ok,
    NoMatch,
    RuntimeError,
}

fn query_spans_json(module: &Module) -> Value {
    let spans = module
        .spans()
        .iter()
        .enumerate()
        .map(|(id, span)| query_span_json(id, span))
        .collect::<Vec<_>>();
    Value::Array(spans)
}

fn query_span_json(id: usize, span: SpanEntry) -> Value {
    let (kind, labeling) = query_span_kind(span.kind);
    let mut object = Map::new();
    object.insert("id".to_string(), json!(id));
    object.insert("source_id".to_string(), json!(span.source_id));
    object.insert("kind".to_string(), json!(kind));
    if let Some(labeling) = labeling {
        object.insert("labeling".to_string(), json!(labeling));
    }
    object.insert("span".to_string(), json!([span.start, span.end]));
    if span.type_id != SPAN_NO_BINDING {
        let mut binding = Map::new();
        binding.insert("type_id".to_string(), json!(span.type_id));
        if span.member != SPAN_NO_BINDING {
            binding.insert("member_id".to_string(), json!(span.member));
        }
        object.insert("binding".to_string(), Value::Object(binding));
    }
    Value::Object(object)
}

fn query_span_kind(kind: SpanKind) -> (&'static str, Option<&'static str>) {
    match kind {
        SpanKind::Def => ("definition", None),
        SpanKind::Ref => ("reference", None),
        SpanKind::Pattern => ("pattern", None),
        SpanKind::Capture => ("capture", None),
        SpanKind::GrammarField => ("grammar_field", None),
        SpanKind::NegatedGrammarField => ("negated_grammar_field", None),
        SpanKind::Predicate => ("predicate", None),
        SpanKind::Quantifier => ("quantifier", None),
        SpanKind::Sequence => ("sequence", None),
        SpanKind::Alternation(Labeling::Unlabeled) => ("alternation", Some("unlabeled")),
        SpanKind::Alternation(Labeling::Labeled) => ("alternation", Some("labeled")),
        SpanKind::Alternative => ("alternative", None),
        SpanKind::CaptureType => ("capture_type", None),
    }
}

fn entry_point_names(module: &Module) -> Vec<String> {
    module.entry_point_names().map(str::to_string).collect()
}

fn runtime_error_value(error: &RuntimeError) -> Value {
    let rendered = render_runtime_error(error, true);
    serde_json::from_str(&rendered).unwrap_or(Value::String(rendered))
}

fn print_summary(bundle: &Value, color: bool) {
    let colors = Colors::new(color);
    let span_count = bundle
        .get("query_spans")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let entry_points = bundle
        .get("entry_points")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    println!("query spans: {span_count}");
    println!("entry points: {entry_points}");
    if let Some(error) = bundle.get("error") {
        eprintln!("error: {error}");
    }
    if let Some(result) = bundle.get("result")
        && !result.is_null()
    {
        println!("result: {}", result);
    }
    if let Some(diagnostics) = bundle.get("diagnostics").and_then(Value::as_array)
        && !diagnostics.is_empty()
    {
        eprintln!(
            "{}diagnostics: {}{}",
            colors.dim,
            diagnostics.len(),
            colors.reset
        );
    }
}
