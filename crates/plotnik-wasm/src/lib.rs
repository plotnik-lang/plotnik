//! WebAssembly bindings for playground and editor integrations.
//!
//! A thin shim over `plotnik_lib`, in three modules:
//! - `lib.rs` — the `#[wasm_bindgen]` surface (`Session`, `ast`, `tokenize`)
//!   and query execution
//! - `langs` — the feature-gated language registry
//! - `wire` — every JSON payload that crosses to JS, mirrored by the
//!   playground's `protocol.ts`
//!
//! The API is session-based and fine-grained on purpose (compile per query
//! edit, run per source edit, trace only when a debugger asks) — see
//! `docs/wip/playground-design.md` §6.
//!
//! Boundary rule: query text, source text, and language names are user
//! input — failures come back as `{error}` payloads or a rejected `compile`,
//! never a panic. Past that validation the engine is trusted and asserts.

#[cfg(target_family = "wasm")]
mod libc_shims;

mod langs;
mod wire;

use langs::Lang;
use plotnik_lib::bytecode::{Entrypoint, Module};
use plotnik_lib::{
    MatcherConfig, NoopTracer, QueryBuilder, RecordingTracer, RuntimeLimitSpec, TypeScriptConfig,
    VM, dump_tree, tokenize as query_tokenize,
};
use serde_json::{Value as JsonValue, json};
use wasm_bindgen::prelude::*;
use wire::{InfoParts, error_json, info_json, json_value, result_json, to_js};

/// A compiled query bound to a language: the unit the playground holds on to
/// between keystrokes. Compiling replaces the whole session; `run`/`trace`
/// execute it against fresh source text.
#[wasm_bindgen]
pub struct Session {
    lang: &'static Lang,
    module: Option<Module>,
    entrypoints: Vec<String>,
    info: JsonValue,
    generated_rust: Option<String>,
}

#[wasm_bindgen]
impl Session {
    /// Compile a query with inspection enabled.
    ///
    /// Ordinary query errors still produce a session (diagnostics inside
    /// `info`, no module); `Err` is reserved for pathological failures.
    pub fn compile(query: &str, lang: &str) -> Result<Session, JsValue> {
        let lang = langs::resolve(lang).map_err(|error| JsValue::from_str(&error))?;
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
        let entrypoints = compiled.module().map(entrypoint_names).unwrap_or_default();
        let identity = lang.identity();
        let generated_rust =
            compiled.to_rust_matcher(MatcherConfig::new().grammar_identity(identity));
        let info = info_json(InfoParts {
            module: compiled.module(),
            tokens: json_value!(tokens),
            diagnostics,
            dts,
            dts_map: json_value!(dts_map),
            entrypoints: &entrypoints,
            bytecode_size,
        });
        let module = compiled.into_module();

        Ok(Session {
            lang,
            module,
            entrypoints,
            info,
            generated_rust,
        })
    }

    pub fn info(&self) -> JsValue {
        to_js(&self.info)
    }

    /// Generate the production Rust matcher for this query. Ordinary query
    /// diagnostics return `code: null`; the accompanying identity always says
    /// which exact embedded grammar a successful module links against.
    pub fn generate(&self, target: &str) -> Result<JsValue, JsValue> {
        if target != "rust" {
            return Err(JsValue::from_str("generation target must be 'rust'"));
        }
        let identity = self.lang.identity();
        Ok(to_js(&json!({
            "target": "rust",
            "code": self.generated_rust,
            "grammar": {
                "name": identity.name(),
                "sha256": identity.sha256(),
                "source": identity.source(),
            },
        })))
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

/// Render `source`'s tree as a query-shaped dump plus the node table mapping
/// the dump back to source ranges (`AstResult` in protocol.ts).
#[wasm_bindgen]
pub fn ast(source: &str, lang: &str, raw: bool) -> JsValue {
    let value = match langs::resolve(lang) {
        Ok(lang) => {
            let tree = lang.parse_source(source);
            let dump = dump_tree(&tree, source, lang.grammar(), raw);
            json!({ "chunks": json_value!(dump.chunks), "nodes": json_value!(dump.nodes) })
        }
        Err(error) => error_json(error),
    };
    to_js(&value)
}

/// Lex a query for editor highlighting; total over arbitrary input.
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
        let vm = VM::builder(source, &tree)
            .limits(RuntimeLimitSpec::default())
            .build();
        match trace {
            TraceMode::None => {
                let mut tracer = NoopTracer;
                let result = vm.execute_with_stats(module, &entrypoint, &mut tracer);
                result_json(module, &entrypoint, source, result, None)
            }
            TraceMode::Recording(max_records) => {
                let mut tracer = RecordingTracer::new(module, max_records);
                let result = vm.execute_with_stats(module, &entrypoint, &mut tracer);
                let recording = tracer.finish();
                result_json(
                    module,
                    &entrypoint,
                    source,
                    result,
                    Some(json_value!(recording)),
                )
            }
        }
    }
}

enum TraceMode {
    None,
    Recording(usize),
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

fn entrypoint_names(module: &Module) -> Vec<String> {
    module.entrypoint_names().map(str::to_string).collect()
}
