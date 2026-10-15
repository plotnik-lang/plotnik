use std::fs;

use plotnik_compiler::bytecode::type_system::TYPE_NO_VALUE;
use plotnik_compiler::grammar::{Grammar, raw::RawGrammar};
use plotnik_compiler::{
    BytecodeConfig, CodegenProvenance, GrammarIdentity, QueryBuilder, RustCodegenConfig,
};

use crate::corpus::Case;

pub(crate) fn source(case: &Case) -> Result<(String, bool), String> {
    let bytes = fs::read(&case.grammar_json)
        .map_err(|error| format!("read grammar {}: {error}", case.grammar_json.display()))?;
    let json = std::str::from_utf8(&bytes).map_err(|error| {
        format!(
            "grammar {} is not UTF-8: {error}",
            case.grammar_json.display()
        )
    })?;
    let raw = RawGrammar::from_json(json)
        .map_err(|error| format!("parse grammar {}: {error:?}", case.grammar_json.display()))?;
    let identity = GrammarIdentity::from_json_bytes(
        raw.name.clone(),
        &bytes,
        case.grammar_json.display().to_string(),
    );
    let grammar = Grammar::from_raw(&raw)
        .map_err(|error| format!("load grammar {}: {error:?}", case.grammar_json.display()))?
        .with_identity(identity);
    let (sources, expression) = case.fixture.collection();
    assert!(expression.is_none(), "static corpus excludes expressions");
    let compiled = QueryBuilder::from_sources(sources)
        .compile(&grammar)
        .map_err(|error| format!("compile `{}`: {error}", case.relative))?;
    if !compiled.diagnostics().is_empty() {
        eprint!(
            "{}",
            compiled
                .diagnostics()
                .render_colored(compiled.source_map(), false)
        );
    }
    if compiled.diagnostics().has_errors() {
        return Err(format!("compilation failed for `{}`", case.relative));
    }

    // The VM corpus renders match-only success as null. Project only that
    // presentation difference, using the query's result type, not its expected output.
    let module = compiled
        .emit(BytecodeConfig::new())
        .map_err(|error| format!("emit corpus metadata for `{}`: {error}", case.relative))?
        .into_artifact()
        .expect("valid corpus query emits bytecode");
    let match_only = module
        .entrypoint("Q")
        .expect("corpus entry Q exists")
        .result_type()
        == TYPE_NO_VALUE.into();

    let emission = compiled
        .emit(
            RustCodegenConfig::new()
                .serde(true)
                .provenance(CodegenProvenance::Full),
        )
        .map_err(|error| format!("emit Rust matcher for `{}`: {error}", case.relative))?;
    if !emission.diagnostics().is_empty() {
        eprint!(
            "{}",
            emission
                .diagnostics()
                .render_colored(compiled.source_map(), false)
        );
    }
    if emission.diagnostics().has_errors() {
        return Err(format!("Rust emission failed for `{}`", case.relative));
    }

    emission
        .into_artifact()
        .map(|artifact| (artifact.into_source(), match_only))
        .ok_or_else(|| format!("Rust emission produced no matcher for `{}`", case.relative))
}
