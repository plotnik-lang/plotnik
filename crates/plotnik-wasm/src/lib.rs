//! WebAssembly bindings for playground and editor integrations.
//!
//! A thin shim over `plotnik_lib`, in three modules:
//! - `lib.rs` — the `#[wasm_bindgen]` surface (`Session`, `tree`, `tokenize`)
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

#[cfg(test)]
mod wire_tests;

use std::cell::OnceCell;

use langs::Lang;
use plotnik_lib::bytecode::{EntryPoint, Module};
use plotnik_lib::{
    BytecodeConfig, BytecodeInspection, CodegenProvenance, CompiledQuery, NoopTracer, QueryBuilder,
    RecordingTracer, RuntimeLimitSpec, RustCodegenConfig, TypeScriptCodegenConfig, VM, dump_tree,
    tokenize as query_tokenize,
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
    compiled: CompiledQuery,
    module: Option<Module>,
    entrypoints: Vec<String>,
    info: JsonValue,
    generated_rust: OnceCell<Option<String>>,
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
            .compile(lang.grammar())
            .map_err(|error| JsValue::from_str(&error.to_string()))?;

        let bytecode = compiled
            .emit(BytecodeConfig::new().inspection(BytecodeInspection::Spans))
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let bytecode_diagnostics = bytecode.diagnostics().clone();
        let mut diagnostics = compiled.diagnostics().clone();
        let module = bytecode.into_artifact();
        let bytecode_size_bytes = module.as_ref().map(Module::bytecode_size);
        let types = compiled
            .emit_types(TypeScriptCodegenConfig::new().colored(false))
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        diagnostics.extend(types.diagnostics().clone());
        diagnostics.extend(bytecode_diagnostics);
        let diagnostics = json_value!(diagnostics.to_wire(compiled.source_map()));
        let (typescript_declarations, typescript_bindings) = types
            .into_artifact()
            .map(|output| output.into_parts())
            .unwrap_or_else(|| (String::new(), Vec::new()));
        let entrypoints = module.as_ref().map(entrypoint_names).unwrap_or_default();
        let info = info_json(InfoParts {
            module: module.as_ref(),
            query_tokens: json_value!(tokens),
            diagnostics,
            typescript_declarations,
            typescript_bindings: json_value!(typescript_bindings),
            entry_points: &entrypoints,
            bytecode_size_bytes,
        });

        Ok(Session {
            lang,
            compiled,
            module,
            entrypoints,
            info,
            generated_rust: OnceCell::new(),
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
        let code = self.generated_rust.get_or_init(|| {
            self.compiled
                .emit(RustCodegenConfig::new().provenance(CodegenProvenance::Full))
                .expect("built-in Rust emission configuration is valid")
                .into_artifact()
                .map(|output| output.into_source())
        });
        Ok(to_js(&json!({
            "target": "rust",
            "code": code,
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
/// the dump back to source ranges (`TreeResult` in protocol.ts).
#[wasm_bindgen]
pub fn tree(source: &str, lang: &str, include_anonymous: bool) -> JsValue {
    let value = match langs::resolve(lang) {
        Ok(lang) => {
            let tree = lang.parse_source(source);
            let dump = dump_tree(&tree, source, lang.grammar(), include_anonymous);
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
) -> Result<EntryPoint, String> {
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
