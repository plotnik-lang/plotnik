use std::fs;
use std::path::PathBuf;

use plotnik_compiler::bytecode::{TypeDefKind, TypeKind};
use plotnik_compiler::grammar::{Grammar, raw::RawGrammar};
use plotnik_compiler::{
    BytecodeConfig, CodegenProvenance, GrammarIdentity, QueryBuilder, RustCodegenConfig, SourceMap,
};

use crate::corpus::Case;

pub(crate) struct RustSource {
    pub(crate) path: PathBuf,
    pub(crate) module_name: String,
    pub(crate) source: String,
}

pub(crate) struct RustSources {
    pub(crate) files: Vec<RustSource>,
    pub(crate) entry_module: String,
    pub(crate) match_only: bool,
}

pub(crate) fn source(case: &Case) -> Result<RustSources, String> {
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

    let module = compiled
        .emit(BytecodeConfig::new())
        .map_err(|error| format!("emit corpus metadata for `{}`: {error}", case.relative))?
        .into_artifact()
        .expect("valid corpus query emits bytecode");
    let entry = module.entrypoint("Q").expect("corpus entry Q exists");
    let result_type = module
        .types()
        .get(entry.result_type())
        .expect("entrypoint result type belongs to its module");
    let match_only = matches!(
        result_type.decode(),
        TypeDefKind::Primitive(TypeKind::NoValue)
    );

    let emission = compiled
        .emit_rust_sources(
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

    let artifacts = emission
        .into_artifact()
        .ok_or_else(|| format!("Rust emission produced no matcher for `{}`", case.relative))?;
    let mut files = Vec::with_capacity(artifacts.len());
    let mut entry_module = None;
    for artifact in artifacts {
        let source_id = artifact.source_id();
        let path = compiled
            .source_map()
            .path(source_id)
            .expect("corpus query sources have filenames");
        // Parse each file independently to locate Q without inspecting generated Rust.
        // Cross-file references may be unresolved here; definition names still exist.
        let query = QueryBuilder::new(SourceMap::from_inline(
            compiled.source_map().content(source_id),
        ))
        .analyze()
        .map_err(|error| format!("locate corpus entry in `{path}`: {error}"))?;
        if query.definition_names().any(|name| name == "Q") {
            assert!(entry_module.is_none(), "corpus entry Q has one owner");
            entry_module = Some(artifact.module_name().to_owned());
        }
        files.push(RustSource {
            path: PathBuf::from(path).with_extension("rs"),
            module_name: artifact.module_name().to_owned(),
            source: artifact.source().to_owned(),
        });
    }
    Ok(RustSources {
        files,
        entry_module: entry_module.expect("corpus entry Q belongs to an emitted source"),
        match_only,
    })
}
