//! WebAssembly bindings for playground and editor integrations.

#[cfg(any(feature = "lang-javascript", feature = "lang-typescript"))]
use std::sync::LazyLock;

use arborium_tree_sitter::{Language, Parser, Tree};
use plotnik_lib::bytecode::{Entrypoint, Module, SPAN_NO_BINDING};
use plotnik_lib::grammar::Grammar;
#[cfg(any(feature = "lang-javascript", feature = "lang-typescript"))]
use plotnik_lib::grammar::raw::RawGrammar;
use plotnik_lib::{
    Colors, NoopTracer, QueryBuilder, RecordingTracer, RuntimeError, RuntimeLimitSpec,
    TypeScriptConfig, VM, extract_inspection, materialize_verified, tokenize as query_tokenize,
    tree_to_json,
};
use serde_json::{Map, Value as JsonValue, json};
use wasm_bindgen::prelude::*;

macro_rules! json_value {
    ($value:expr) => {
        serde_json::to_value($value).expect("wasm bundle value serializes")
    };
}

#[wasm_bindgen]
pub struct Session {
    lang: &'static Lang,
    module: Option<Module>,
    entrypoints: Vec<String>,
    info: JsonValue,
}

#[wasm_bindgen]
impl Session {
    /// Compile a query with inspection enabled.
    pub fn compile(query: &str, lang: &str) -> Result<Session, JsValue> {
        let lang = resolve_lang(lang).map_err(|error| JsValue::from_str(&error))?;
        let tokens = query_tokenize(query);
        let compiled = QueryBuilder::from_inline(query)
            .with_inspection(true)
            .compile(lang.grammar())
            .map_err(|error| JsValue::from_str(&error.to_string()))?;

        let diagnostics = json_value!(compiled.diagnostics().to_wire(compiled.source_map()));
        let bytecode_size = compiled.bytecode().map(<[u8]>::len);
        let (dts, dts_map) = compiled
            .to_typescript_mapped(TypeScriptConfig::new().colored(false))
            .unwrap_or_else(|| (String::new(), Vec::new()));
        let spans = compiled
            .module()
            .map(spans_json)
            .unwrap_or_else(|| JsonValue::Array(Vec::new()));
        let entrypoints = compiled.module().map(entrypoint_names).unwrap_or_default();
        let info = info_json(InfoParts {
            spans,
            tokens: json_value!(tokens),
            diagnostics,
            dts,
            dts_map: json_value!(dts_map),
            entrypoints: json_value!(&entrypoints),
            bytecode_size,
        });
        let module = compiled.into_module();

        Ok(Session {
            lang,
            module,
            entrypoints,
            info,
        })
    }

    pub fn info(&self) -> JsValue {
        to_js(&self.info)
    }

    /// Run the compiled query against source text.
    pub fn run(&self, source: &str, entry: Option<String>) -> JsValue {
        let value = self.execute(source, entry.as_deref(), TraceMode::None);
        to_js(&value)
    }

    /// Run with a bounded execution recording.
    pub fn trace(&self, source: &str, entry: Option<String>, max_records: u32) -> JsValue {
        let max_records = usize::try_from(max_records).expect("u32 fits usize");
        let value = self.execute(source, entry.as_deref(), TraceMode::Recording(max_records));
        to_js(&value)
    }
}

#[wasm_bindgen]
pub fn ast(source: &str, lang: &str, raw: bool) -> JsValue {
    let value = match resolve_lang(lang) {
        Ok(lang) => {
            let tree = lang.parse_source(source);
            tree_to_json(&tree, source, raw)
        }
        Err(error) => error_json(error),
    };
    to_js(&value)
}

#[wasm_bindgen(js_name = tokenize)]
pub fn tokenize_js(query: &str) -> JsValue {
    to_js(&json_value!(query_tokenize(query)))
}

impl Session {
    fn execute(&self, source: &str, entry: Option<&str>, trace: TraceMode) -> JsonValue {
        let Some(module) = self.module.as_ref() else {
            return error_json("query did not compile");
        };

        let entrypoint = match resolve_entrypoint(module, entry, &self.entrypoints) {
            Ok(entrypoint) => entrypoint,
            Err(error) => return error_json(error),
        };

        let tree = self.lang.parse_source(source);
        match trace {
            TraceMode::None => run_module(module, &entrypoint, source, &tree),
            TraceMode::Recording(max_records) => {
                trace_module(module, &entrypoint, source, &tree, max_records)
            }
        }
    }
}

enum TraceMode {
    None,
    Recording(usize),
}

struct Lang {
    #[cfg(any(feature = "lang-javascript", feature = "lang-typescript"))]
    name: &'static str,
    #[cfg(any(feature = "lang-javascript", feature = "lang-typescript"))]
    aliases: &'static [&'static str],
    ts_language: Language,
    grammar: fn() -> &'static Grammar,
}

impl Lang {
    #[cfg(any(feature = "lang-javascript", feature = "lang-typescript"))]
    fn new(
        name: &'static str,
        aliases: &'static [&'static str],
        ts_language: Language,
        grammar: fn() -> &'static Grammar,
    ) -> Self {
        Self {
            name,
            aliases,
            ts_language,
            grammar,
        }
    }

    #[cfg(any(feature = "lang-javascript", feature = "lang-typescript"))]
    fn matches(&self, candidate: &str) -> bool {
        self.name == candidate || self.aliases.contains(&candidate)
    }

    fn grammar(&self) -> &Grammar {
        (self.grammar)()
    }

    fn parse_source(&self, source: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&self.ts_language)
            .expect("failed to set language");
        parser.parse(source, None).expect("failed to parse source")
    }
}

#[cfg(feature = "lang-javascript")]
static JAVASCRIPT_GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
    load_grammar(
        include_str!(env!("PLOTNIK_WASM_GRAMMAR_JSON_JAVASCRIPT")),
        "javascript",
    )
});

#[cfg(feature = "lang-javascript")]
fn javascript_grammar() -> &'static Grammar {
    &JAVASCRIPT_GRAMMAR
}

#[cfg(feature = "lang-javascript")]
fn javascript() -> &'static Lang {
    static LANGUAGE: LazyLock<Lang> = LazyLock::new(|| {
        Lang::new(
            "javascript",
            &["javascript", "js", "jsx", "ecmascript", "es"],
            arborium_javascript::language().into(),
            javascript_grammar,
        )
    });
    &LANGUAGE
}

#[cfg(feature = "lang-typescript")]
static TYPESCRIPT_GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
    load_grammar(
        include_str!(env!("PLOTNIK_WASM_GRAMMAR_JSON_TYPESCRIPT")),
        "typescript",
    )
});

#[cfg(feature = "lang-typescript")]
fn typescript_grammar() -> &'static Grammar {
    &TYPESCRIPT_GRAMMAR
}

#[cfg(feature = "lang-typescript")]
fn typescript() -> &'static Lang {
    static LANGUAGE: LazyLock<Lang> = LazyLock::new(|| {
        Lang::new(
            "typescript",
            &["typescript", "ts"],
            arborium_typescript::language().into(),
            typescript_grammar,
        )
    });
    &LANGUAGE
}

fn resolve_lang(input: &str) -> Result<&'static Lang, String> {
    #[cfg(any(feature = "lang-javascript", feature = "lang-typescript"))]
    let candidate = input.to_ascii_lowercase();

    #[cfg(feature = "lang-javascript")]
    if javascript().matches(&candidate) {
        return Ok(javascript());
    }

    #[cfg(feature = "lang-typescript")]
    if typescript().matches(&candidate) {
        return Ok(typescript());
    }

    let supported = supported_language_names();
    if supported.is_empty() {
        return Err("no languages are enabled in this plotnik-wasm build".to_string());
    }
    Err(format!(
        "unsupported language: {input}; supported languages: {}",
        supported.join(", ")
    ))
}

fn supported_language_names() -> Vec<&'static str> {
    vec![
        #[cfg(feature = "lang-javascript")]
        "javascript",
        #[cfg(feature = "lang-typescript")]
        "typescript",
    ]
}

#[cfg(any(feature = "lang-javascript", feature = "lang-typescript"))]
fn load_grammar(json: &str, name: &str) -> Grammar {
    let raw = RawGrammar::from_json(json)
        .unwrap_or_else(|error| panic!("invalid embedded {name} grammar JSON: {error}"));
    Grammar::from_raw(&raw)
        .unwrap_or_else(|error| panic!("invalid embedded {name} grammar metadata: {error}"))
}

struct InfoParts {
    spans: JsonValue,
    tokens: JsonValue,
    diagnostics: JsonValue,
    dts: String,
    dts_map: JsonValue,
    entrypoints: JsonValue,
    bytecode_size: Option<usize>,
}

fn info_json(parts: InfoParts) -> JsonValue {
    let mut object = Map::new();
    object.insert("v".to_string(), json!(1));
    object.insert("spans".to_string(), parts.spans);
    object.insert("tokens".to_string(), parts.tokens);
    object.insert("diagnostics".to_string(), parts.diagnostics);
    object.insert("dts".to_string(), JsonValue::String(parts.dts));
    object.insert("dts_map".to_string(), parts.dts_map);
    object.insert("entrypoints".to_string(), parts.entrypoints);
    object.insert("bytecode_size".to_string(), json!(parts.bytecode_size));
    JsonValue::Object(object)
}

fn run_module(module: &Module, entrypoint: &Entrypoint, source: &str, tree: &Tree) -> JsonValue {
    let vm = VM::builder(source, tree)
        .limits(RuntimeLimitSpec::default())
        .build();
    let mut tracer = NoopTracer;
    let result = vm.execute_with_stats(module, entrypoint, &mut tracer);
    result_json(module, entrypoint, source, result, None)
}

fn trace_module(
    module: &Module,
    entrypoint: &Entrypoint,
    source: &str,
    tree: &Tree,
    max_records: usize,
) -> JsonValue {
    let vm = VM::builder(source, tree)
        .limits(RuntimeLimitSpec::default())
        .build();
    let mut tracer = RecordingTracer::new(module, max_records);
    let result = vm.execute_with_stats(module, entrypoint, &mut tracer);
    let recording = tracer.finish();
    result_json(
        module,
        entrypoint,
        source,
        result,
        Some(json_value!(recording)),
    )
}

fn result_json(
    module: &Module,
    entrypoint: &Entrypoint,
    source: &str,
    result: (
        Result<plotnik_lib::EffectLog<'_>, RuntimeError>,
        plotnik_lib::RunStats,
    ),
    trace: Option<JsonValue>,
) -> JsonValue {
    let (result, stats) = result;
    match result {
        Ok(effects) => {
            let colors = Colors::new(false);
            let value =
                materialize_verified(source, module, entrypoint, effects.as_slice(), colors);
            let inspection = (!module.spans().is_empty())
                .then(|| extract_inspection(effects.as_slice(), module));

            let mut object = Map::new();
            object.insert("value".to_string(), json_value!(value));
            object.insert("inspection".to_string(), json_value!(inspection));
            object.insert("stats".to_string(), json_value!(stats));
            if let Some(trace) = trace {
                object.insert("trace".to_string(), trace);
            }
            JsonValue::Object(object)
        }
        Err(RuntimeError::NoMatch) => runtime_error_json("no match", trace),
        Err(error) => runtime_error_json(error.to_string(), trace),
    }
}

fn runtime_error_json(error: impl Into<String>, trace: Option<JsonValue>) -> JsonValue {
    let mut object = Map::new();
    object.insert("error".to_string(), JsonValue::String(error.into()));
    if let Some(trace) = trace {
        object.insert("trace".to_string(), trace);
    }
    JsonValue::Object(object)
}

fn error_json(error: impl Into<String>) -> JsonValue {
    let mut object = Map::new();
    object.insert("error".to_string(), JsonValue::String(error.into()));
    JsonValue::Object(object)
}

fn resolve_entrypoint(
    module: &Module,
    requested: Option<&str>,
    entrypoints: &[String],
) -> Result<Entrypoint, String> {
    let selected = match requested {
        Some(name) => name.to_string(),
        None => entrypoints
            .last()
            .cloned()
            .ok_or_else(|| "no entrypoints in module".to_string())?,
    };

    let Some(entrypoint) = module.entrypoint(&selected) else {
        return Err(format!(
            "invalid entrypoint: {}; available entrypoints: {}",
            selected,
            entrypoints.join(", ")
        ));
    };
    Ok(entrypoint)
}

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

fn entrypoint_names(module: &Module) -> Vec<String> {
    module.entrypoint_names().map(str::to_string).collect()
}

fn to_js(value: &JsonValue) -> JsValue {
    serde_wasm_bindgen::to_value(value).expect("JSON value converts to JsValue")
}
