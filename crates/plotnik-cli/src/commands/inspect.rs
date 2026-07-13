//! One-shot compile/run bundle for playground and editor integrations.

use std::path::PathBuf;

use plotnik_lib::bytecode::{Module, SPAN_NO_BINDING};
use plotnik_lib::{
    BytecodeConfig, BytecodeInspection, Colors, NoopTracer, QueryBuilder, RecordingTracer,
    RuntimeError, RuntimeLimitSpec, TypeScriptCodegenConfig, VM, extract_inspection,
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
    let tokens = tokenize(source.content);
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
    let (dts, dts_map) = types
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
    let spans = module
        .as_ref()
        .map(spans_json)
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let entrypoints = module.as_ref().map(entrypoint_names).unwrap_or_default();

    let run = if let Some(module) = module.as_ref() {
        let default_entry = module.entrypoint_names().last().map(str::to_owned);
        let entry = args.entry.clone().or(shebang_entry).or(default_entry);
        let entrypoint = run_common::resolve_entrypoint(module, entry.as_deref())?;
        let tree = lang.parse_source(&source_code);
        run_module(
            module,
            &entrypoint,
            &source_code,
            &tree,
            args.limits,
            args.trace,
        )
    } else {
        RunPayload::not_run()
    };

    let bundle = bundle_json(BundleParts {
        spans,
        tokens: json_value!(tokens),
        diagnostics: json_value!(diagnostics),
        dts,
        dts_map: json_value!(dts_map),
        entrypoints: json_value!(entrypoints),
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
    spans: Value,
    tokens: Value,
    diagnostics: Value,
    dts: String,
    dts_map: Value,
    entrypoints: Value,
    run: &'a RunPayload,
}

fn bundle_json(parts: BundleParts<'_>) -> Value {
    let mut object = Map::new();
    object.insert("v".to_string(), json!(1));
    object.insert("spans".to_string(), parts.spans);
    object.insert("tokens".to_string(), parts.tokens);
    object.insert("diagnostics".to_string(), parts.diagnostics);
    object.insert("dts".to_string(), Value::String(parts.dts));
    object.insert("dts_map".to_string(), parts.dts_map);
    object.insert("entrypoints".to_string(), parts.entrypoints);
    object.insert("value".to_string(), parts.run.value.clone());
    object.insert("inspection".to_string(), parts.run.inspection.clone());
    object.insert("stats".to_string(), parts.run.stats.clone());
    object.insert("trace".to_string(), parts.run.trace.clone());
    if let Some(error) = &parts.run.error {
        object.insert("error".to_string(), error.clone());
    }
    Value::Object(object)
}

fn run_module(
    module: &Module,
    entrypoint: &plotnik_lib::bytecode::Entrypoint,
    source_code: &str,
    tree: &tree_sitter::Tree,
    limits: RuntimeLimitSpec,
    trace: bool,
) -> RunPayload {
    let vm = VM::builder(source_code, tree).limits(limits).build();
    if trace {
        let mut tracer = RecordingTracer::new(module, DEFAULT_MAX_RECORDS);
        let (result, stats) = vm.execute_with_stats(module, entrypoint, &mut tracer);
        let recording = tracer.finish();
        return run_payload_from_result(
            module,
            entrypoint,
            source_code,
            (result, stats),
            Some(json_value!(recording)),
        );
    }

    let mut tracer = NoopTracer;
    let (result, stats) = vm.execute_with_stats(module, entrypoint, &mut tracer);
    run_payload_from_result(module, entrypoint, source_code, (result, stats), None)
}

fn run_payload_from_result(
    module: &Module,
    entrypoint: &plotnik_lib::bytecode::Entrypoint,
    source_code: &str,
    result: (
        Result<plotnik_lib::MatchJournal<'_>, RuntimeError>,
        plotnik_lib::RunStats,
    ),
    trace: Option<Value>,
) -> RunPayload {
    let (result, stats) = result;
    match result {
        Ok(journal) => {
            let colors = Colors::new(false);
            let value =
                materialize_verified(source_code, module, entrypoint, journal.as_slice(), colors);
            let inspection = (!module.spans().is_empty())
                .then(|| extract_inspection(journal.as_slice(), module));
            RunPayload {
                value: json_value!(value),
                inspection: json_value!(inspection),
                stats: json_value!(stats),
                trace: trace.unwrap_or(Value::Null),
                error: None,
                exit: InspectExit::Ok,
            }
        }
        Err(RuntimeError::NoMatch) => RunPayload {
            value: Value::Null,
            inspection: Value::Null,
            stats: Value::Null,
            trace: trace.unwrap_or(Value::Null),
            error: Some(Value::String("no match".to_string())),
            exit: InspectExit::NoMatch,
        },
        Err(error) => RunPayload {
            value: Value::Null,
            inspection: Value::Null,
            stats: Value::Null,
            trace: trace.unwrap_or(Value::Null),
            error: Some(runtime_error_value(&error)),
            exit: InspectExit::RuntimeError,
        },
    }
}

struct RunPayload {
    value: Value,
    inspection: Value,
    stats: Value,
    trace: Value,
    error: Option<Value>,
    exit: InspectExit,
}

impl RunPayload {
    fn not_run() -> Self {
        Self {
            value: Value::Null,
            inspection: Value::Null,
            stats: Value::Null,
            trace: Value::Null,
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

fn spans_json(module: &Module) -> Value {
    let spans = module
        .spans()
        .iter()
        .enumerate()
        .map(|(id, span)| {
            json!({
                "id": id,
                "source": span.source,
                "kind": span.kind.name(),
                "start": span.start,
                "end": span.end,
                "type": binding_value(span.type_id),
                "member": binding_value(span.member),
            })
        })
        .collect::<Vec<_>>();
    Value::Array(spans)
}

fn binding_value(value: u16) -> Value {
    if value == SPAN_NO_BINDING {
        Value::Null
    } else {
        json!(value)
    }
}

fn entrypoint_names(module: &Module) -> Vec<String> {
    module.entrypoint_names().map(str::to_string).collect()
}

fn runtime_error_value(error: &RuntimeError) -> Value {
    let rendered = render_runtime_error(error, true);
    serde_json::from_str(&rendered).unwrap_or(Value::String(rendered))
}

fn print_summary(bundle: &Value, color: bool) {
    let colors = Colors::new(color);
    let span_count = bundle
        .get("spans")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let entrypoints = bundle
        .get("entrypoints")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    println!("spans: {span_count}");
    println!("entrypoints: {entrypoints}");
    if let Some(error) = bundle.get("error") {
        eprintln!("error: {error}");
    }
    if let Some(value) = bundle.get("value")
        && !value.is_null()
    {
        println!("value: {}", value);
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
