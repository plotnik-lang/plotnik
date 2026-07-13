//! JSON payloads crossing the wasm boundary.
//!
//! Single source of truth for the wire shapes, mirrored by hand in the
//! playground's `web/src/components/playground/protocol.ts` — change the two
//! together. Payloads are assembled as `serde_json::Value` and handed to JS
//! through [`to_js`] (plain objects, not the ES Maps serde-wasm-bindgen
//! defaults to).
//!
//! Offsets in any payload are byte offsets into the exact text the caller
//! passed in; the web side converts to UTF-16 at its edge (`byte-offsets.ts`).

use plotnik_lib::bytecode::{Entrypoint, Module, SPAN_NO_BINDING, SpanEntry, SpanKind};
use plotnik_lib::{
    Colors, MatchJournal, RunStats, RuntimeError, extract_result_provenance, materialize_verified,
};
use serde_json::{Map, Value as JsonValue, json};
use wasm_bindgen::JsValue;

/// Serialize an engine-produced value, asserting success: everything past
/// query validation is trusted, including its serializability.
macro_rules! json_value {
    ($value:expr) => {
        serde_json::to_value($value).expect("wasm bundle value serializes")
    };
}
pub(crate) use json_value;

/// Inputs for the `Session::info()` payload (`SessionInfo` in protocol.ts).
pub struct InfoParts<'a> {
    /// `None` when the query didn't produce bytecode (query spans come back empty).
    pub module: Option<&'a Module>,
    pub query_tokens: JsonValue,
    pub diagnostics: JsonValue,
    pub typescript_declarations: String,
    pub typescript_bindings: JsonValue,
    pub entry_points: &'a [String],
    pub bytecode_size_bytes: Option<usize>,
}

pub fn info_json(parts: InfoParts) -> JsonValue {
    json!({
        // Version marker for the day the shape needs a breaking change.
        "version": 1,
        "query_spans": parts.module.map(query_spans_json).unwrap_or_else(|| json!([])),
        "query_tokens": parts.query_tokens,
        "diagnostics": parts.diagnostics,
        "typescript_declarations": parts.typescript_declarations,
        "typescript_bindings": parts.typescript_bindings,
        "entry_points": parts.entry_points,
        "bytecode_size_bytes": parts.bytecode_size_bytes,
    })
}

/// A finished run (`RunResult` in protocol.ts): result plus provenance on
/// success, `{error}` otherwise ("no match" included), with the execution
/// trace attached when tracing.
pub fn result_json(
    module: &Module,
    entrypoint: &Entrypoint,
    source: &str,
    result: (Result<MatchJournal<'_>, RuntimeError>, RunStats),
    execution_trace: Option<JsonValue>,
) -> JsonValue {
    let (result, stats) = result;
    let mut out = match result {
        Ok(journal) => {
            let colors = Colors::new(false);
            let result =
                materialize_verified(source, module, entrypoint, journal.as_slice(), colors);
            let result_provenance = (!module.spans().is_empty())
                .then(|| extract_result_provenance(journal.as_slice(), module));
            json!({
                "result": json_value!(result),
                "result_provenance": json_value!(result_provenance),
                "run_stats": json_value!(stats),
            })
        }
        Err(RuntimeError::NoMatch) => error_json("no match"),
        Err(error) => error_json(error.to_string()),
    };
    if let Some(execution_trace) = execution_trace {
        out["execution_trace"] = execution_trace;
    }
    out
}

pub fn error_json(error: impl Into<String>) -> JsonValue {
    json!({ "error": error.into() })
}

/// The static query-span table (`QuerySpan[]` in protocol.ts): the hub the
/// playground joins every view through — see `docs/wip/playground-design.md`
/// §2. The array index is the SpanId.
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
    object.insert("source_id".to_string(), json!(span.source));
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
        SpanKind::Field => ("field", None),
        SpanKind::NegField => ("negated_field", None),
        SpanKind::Predicate => ("predicate", None),
        SpanKind::Quantifier => ("quantifier", None),
        SpanKind::Sequence => ("sequence", None),
        SpanKind::UnlabeledAlternation => ("alternation", Some("unlabeled")),
        SpanKind::LabeledAlternation => ("alternation", Some("labeled")),
        SpanKind::Alternative => ("alternative", None),
        SpanKind::CaptureType => ("capture_type", None),
    }
}

pub fn to_js(value: &JsonValue) -> JsValue {
    use serde::Serialize;

    // The default serializer turns JSON objects into ES Maps; json_compatible
    // produces plain objects, which is what the playground consumes.
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    value
        .serialize(&serializer)
        .expect("JSON value converts to JsValue")
}
