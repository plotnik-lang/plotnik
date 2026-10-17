//! WebAssembly bindings for playground and editor integrations.
//!
//! Payload offsets are UTF-8 byte offsets into the supplied text.

mod langs;
mod wire;

#[cfg(test)]
mod langs_tests;
#[cfg(test)]
mod wire_tests;

use std::cell::OnceCell;

use langs::Lang;
use plotnik_compiler::bytecode::{Entrypoint, Module};
use plotnik_compiler::{
    BytecodeConfig, BytecodeInspection, CodegenProvenance, CompiledQuery, NoopTracer, QueryBuilder,
    RuntimeLimitSpec, RustCodegenConfig, TraceRecorder, VM, dump_tree, tokenize as query_tokenize,
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
        diagnostics.extend(bytecode_diagnostics);
        let diagnostics = json_value!(diagnostics.to_wire(compiled.source_map()));
        let entrypoints = module.as_ref().map(entrypoint_names).unwrap_or_default();
        let info = info_json(InfoParts {
            module: module.as_ref(),
            query_tokens: json_value!(tokens),
            diagnostics,
            entrypoints: &entrypoints,
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

    /// Query diagnostics and metadata. Query spans are empty without bytecode.
    pub fn info(&self) -> JsValue {
        to_js(&self.info)
    }

    /// Generate the production Rust matcher for this query. Ordinary query
    /// diagnostics return `code: null`; the accompanying identity always says
    /// which exact embedded grammar a successful module is bound to.
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
    /// Returns a result with provenance on success, or `{error}` on failure,
    /// including when no match is found.
    pub fn run(&self, source: &str, entry: Option<String>) -> JsValue {
        self.execute(source, entry.as_deref(), TraceMode::None)
    }

    /// Run with a bounded execution trace.
    pub fn trace(&self, source: &str, entry: Option<String>, max_records: u32) -> JsValue {
        let max_records = usize::try_from(max_records).expect("u32 fits usize");
        self.execute(source, entry.as_deref(), TraceMode::Structured(max_records))
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
    fn execute(&self, source: &str, entry: Option<&str>, trace: TraceMode) -> JsValue {
        let Some(module) = self.module.as_ref() else {
            return to_js(&error_json("query did not compile"));
        };

        let entrypoint = match resolve_entrypoint(module, entry, &self.entrypoints) {
            Ok(entrypoint) => entrypoint,
            Err(error) => return to_js(&error_json(error)),
        };

        let tree = self.lang.parse_source(source);
        let vm = VM::builder(source, &tree)
            .limits(RuntimeLimitSpec::default())
            .build();
        let account = match vm.account() {
            Ok(account) => account.clone(),
            Err(error) => return to_js(&error_json(error.to_string())),
        };
        match trace {
            TraceMode::None => {
                let mut tracer = NoopTracer;
                let result = vm.execute_with_stats(module, &entrypoint, &mut tracer);
                result_json(module, &account, result, None)
            }
            TraceMode::Structured(max_records) => {
                let mut tracer = TraceRecorder::new(module, max_records);
                let result = vm.execute_with_stats(module, &entrypoint, &mut tracer);
                let execution_trace = tracer.finish();
                result_json(module, &account, result, Some(execution_trace))
            }
        }
    }
}

enum TraceMode {
    None,
    Structured(usize),
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
            .ok_or_else(|| "bytecode module exports no entrypoints".to_string())?,
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
