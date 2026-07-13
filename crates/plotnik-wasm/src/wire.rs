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

use plotnik_lib::bytecode::{Entrypoint, Module, SPAN_NO_BINDING};
use plotnik_lib::{
    Colors, MatchJournal, RunStats, RuntimeError, extract_result_provenance, materialize_verified,
};
use serde_json::{Value as JsonValue, json};
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
    /// `None` when the query didn't produce bytecode (spans come back empty).
    pub module: Option<&'a Module>,
    pub tokens: JsonValue,
    pub diagnostics: JsonValue,
    pub dts: String,
    pub dts_map: JsonValue,
    pub entrypoints: &'a [String],
    pub bytecode_size: Option<usize>,
}

pub fn info_json(parts: InfoParts) -> JsonValue {
    json!({
        // Version marker for the day the shape needs a breaking change.
        "v": 1,
        "spans": parts.module.map(spans_json).unwrap_or_else(|| json!([])),
        "tokens": parts.tokens,
        "diagnostics": parts.diagnostics,
        "dts": parts.dts,
        "dts_map": parts.dts_map,
        "entrypoints": parts.entrypoints,
        "bytecode_size": parts.bytecode_size,
    })
}

/// A finished run (`RunResult` in protocol.ts): materialized value plus
/// inspection on success, `{error}` otherwise ("no match" included), with
/// the recording attached when tracing.
pub fn result_json(
    module: &Module,
    entrypoint: &Entrypoint,
    source: &str,
    result: (Result<MatchJournal<'_>, RuntimeError>, RunStats),
    trace: Option<JsonValue>,
) -> JsonValue {
    let (result, stats) = result;
    let mut out = match result {
        Ok(journal) => {
            let colors = Colors::new(false);
            let value =
                materialize_verified(source, module, entrypoint, journal.as_slice(), colors);
            let result_provenance = (!module.spans().is_empty())
                .then(|| extract_result_provenance(journal.as_slice(), module));
            json!({
                "value": json_value!(value),
                "inspection": json_value!(result_provenance),
                "stats": json_value!(stats),
            })
        }
        Err(RuntimeError::NoMatch) => error_json("no match"),
        Err(error) => error_json(error.to_string()),
    };
    if let Some(trace) = trace {
        out["trace"] = trace;
    }
    out
}

pub fn error_json(error: impl Into<String>) -> JsonValue {
    json!({ "error": error.into() })
}

/// The static span table (`InspectionSpan[]` in protocol.ts): the hub the
/// playground joins every view through — see `docs/wip/playground-design.md`
/// §2. The array index is the SpanId.
fn spans_json(module: &Module) -> JsonValue {
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
    JsonValue::Array(spans)
}

fn binding_value(value: u16) -> JsonValue {
    if value == SPAN_NO_BINDING {
        JsonValue::Null
    } else {
        json!(value)
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
