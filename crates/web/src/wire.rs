use plotnik_compiler::bytecode::{Labeling, Module, SPAN_NO_BINDING, SpanEntry, SpanKind};
use plotnik_compiler::{
    Document, ExecutionTrace, RunStats, RuntimeError, extract_result_provenance,
};
use plotnik_runtime::json::Style;
use plotnik_runtime::{Account, Text};
use serde_json::{Map, Value as JsonValue, json};
use wasm_bindgen::JsValue;

macro_rules! json_value {
    ($value:expr) => {
        serde_json::to_value($value).expect("wasm bundle value serializes")
    };
}
pub(crate) use json_value;

pub struct InfoParts<'a> {
    pub module: Option<&'a Module>,
    pub query_tokens: JsonValue,
    pub diagnostics: JsonValue,
    pub entrypoints: &'a [String],
    pub bytecode_size_bytes: Option<usize>,
}

pub fn info_json(parts: InfoParts) -> JsonValue {
    json!({
        "version": 0,
        "query_spans": parts.module.map(query_spans_json).unwrap_or_else(|| json!([])),
        "query_tokens": parts.query_tokens,
        "diagnostics": parts.diagnostics,
        "entrypoints": parts.entrypoints,
        "bytecode_size_bytes": parts.bytecode_size_bytes,
    })
}

pub fn result_json(
    module: &Module,
    account: &Account,
    result: (Result<Document<'_, '_, '_>, RuntimeError>, RunStats),
    execution_trace: Option<ExecutionTrace<'_>>,
) -> JsValue {
    let (result, mut stats) = result;
    let response = js_sys::Object::new();
    let outcome = match result {
        Ok(document) => {
            let mut output = Text::new(account);
            document
                .write_json(Style::COMPACT, &mut output)
                .map_err(|error| {
                    output
                        .error()
                        .map_or_else(|| error.to_string(), |error| error.to_string())
                })
                .and_then(|()| {
                    let provenance = if module.spans().is_empty() {
                        None
                    } else {
                        Some(
                            extract_result_provenance(&document, module)
                                .map_err(|error| error.to_string())?,
                        )
                    };
                    set(
                        &response,
                        "result",
                        &js_sys::JSON::parse(&output).expect("runtime emits valid JSON"),
                    );
                    set(&response, "result_provenance", &to_js(&provenance));
                    Ok(())
                })
        }
        Err(RuntimeError::NoMatch) => Err("no match".to_string()),
        Err(error) => Err(error.to_string()),
    };
    if let Err(error) = outcome {
        set(&response, "error", &JsValue::from_str(&error));
    }
    if let Some(execution_trace) = execution_trace {
        set(&response, "execution_trace", &to_js(&execution_trace));
    }
    stats.fuel_used = account.fuel_used();
    stats.peak_live_heap_bytes = account.memory_peak();
    set(&response, "run_stats", &to_js(&stats));
    response.into()
}

fn set(object: &js_sys::Object, key: &str, value: &JsValue) {
    js_sys::Reflect::set(object, &JsValue::from_str(key), value)
        .expect("fresh response object accepts fields");
}

pub fn error_json(error: impl Into<String>) -> JsonValue {
    json!({ "error": error.into() })
}

fn query_spans_json(module: &Module) -> JsonValue {
    let spans = module
        .spans()
        .iter()
        .enumerate()
        .map(|(id, span)| query_span_json(id, span))
        .collect::<Vec<_>>();
    JsonValue::Array(spans)
}

pub(super) fn query_span_json(id: usize, span: SpanEntry) -> JsonValue {
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
        object.insert("binding".to_string(), JsonValue::Object(binding));
    }
    JsonValue::Object(object)
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

pub fn to_js(value: &impl serde::Serialize) -> JsValue {
    // The default serializer turns JSON objects into ES Maps; json_compatible
    // produces plain objects, which is what the playground consumes.
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    value
        .serialize(&serializer)
        .expect("JSON value converts to JsValue")
}
