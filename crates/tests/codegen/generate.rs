use std::fs;
use std::path::Path;

use plotnik_compiler::grammar::{Grammar, raw::RawGrammar};
use plotnik_compiler::{
    CodegenProvenance, GrammarIdentity, QueryBuilder, RustCodegenConfig, SourceMap, SourcePath,
};

use crate::corpus::Case;

pub(crate) fn source(case: &Case, query_path: &Path) -> Result<String, String> {
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
    let mut sources = SourceMap::new();
    sources.add_file(SourcePath::new(&query_path.to_string_lossy()), &case.query);
    let compiled = QueryBuilder::new(sources)
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

    let emission = compiled
        .emit(
            RustCodegenConfig::new()
                .debug(true)
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
        .map(|artifact| artifact.into_source())
        .ok_or_else(|| format!("Rust emission produced no matcher for `{}`", case.relative))
}
