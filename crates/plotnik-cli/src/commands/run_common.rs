//! Shared logic for run and trace commands.

use std::fs;
use std::io::{self, Read};
use std::path::Path;

use arborium_tree_sitter as tree_sitter;
use plotnik_lib::QueryBuilder;
use plotnik_lib::bytecode::{Entrypoint, Module};
use plotnik_lib::emit::emit;

use super::lang_resolver::merge_lang;
use super::query_loader::load_query_source;
use crate::error::CliError;
use plotnik::language_registry::{self, Lang};

/// Load source code from file, stdin, or inline text.
pub fn load_source(
    source_text: Option<&str>,
    source_path: Option<&Path>,
    query_path: Option<&Path>,
) -> Result<String, CliError> {
    if let Some(text) = source_text {
        return Ok(text.to_owned());
    }
    if let Some(path) = source_path {
        if path.as_os_str() == "-" {
            if query_path.is_some_and(|p| p.as_os_str() == "-") {
                return Err(CliError::fatal(
                    "query and source cannot both be from stdin",
                ));
            }
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| CliError::fatal(format!("failed to read stdin: {}", e)))?;
            return Ok(buf);
        }
        return fs::read_to_string(path)
            .map_err(|e| CliError::fatal(format!("failed to read '{}': {}", path.display(), e)));
    }
    unreachable!("validation ensures source input exists")
}

/// Resolve the source language.
/// Priority: explicit `-l` (must agree with shebang) > shebang > source extension.
pub fn resolve_lang(
    explicit: Option<&str>,
    declared: Option<&str>,
    source_path: Option<&Path>,
) -> Result<&'static Lang, CliError> {
    if let Some(lang) = merge_lang(explicit, declared)? {
        return Ok(lang);
    }

    if let Some(path) = source_path
        && path.as_os_str() != "-"
        && let Some(ext) = path.extension().and_then(|e| e.to_str())
    {
        if let Some(lang) = language_registry::from_ext(ext) {
            return Ok(lang);
        }
        return Err(CliError::fatal(format!(
            "cannot infer language from extension '.{}', use --lang",
            ext
        )));
    }

    Err(CliError::fatal(
        "--lang is required (cannot infer from input)",
    ))
}

/// Resolve entrypoint by name or use the single available one.
pub fn resolve_entrypoint(module: &Module, name: Option<&str>) -> Result<Entrypoint, CliError> {
    let entries = module.entrypoints();
    let strings = module.strings();

    match name {
        Some(name) => entries
            .find_by_name(name, &strings)
            .ok_or_else(|| CliError::fatal(format!("invalid entrypoint: {}", name))),
        None => {
            if entries.len() == 1 {
                Ok(entries.get(0))
            } else if entries.is_empty() {
                Err(CliError::fatal("no entrypoints in module"))
            } else {
                Err(CliError::fatal(
                    "multiple entrypoints, specify one with --entry",
                ))
            }
        }
    }
}

/// Validate common arguments.
fn validate(
    source_text: Option<&str>,
    source_path: Option<&Path>,
    lang: Option<&str>,
    declared_lang: Option<&str>,
) -> Result<(), CliError> {
    let has_source = source_text.is_some() || source_path.is_some();
    if !has_source {
        return Err(CliError::fatal(
            "source is required: use positional argument or -s/--source",
        ));
    }
    let source_is_inline = source_text.is_some();
    let has_lang = lang.is_some() || declared_lang.is_some();
    if source_is_inline && !has_lang {
        return Err(CliError::fatal("--lang is required when using --source"));
    }
    Ok(())
}

/// Common input parameters for run/trace commands.
pub struct QueryInput<'a> {
    pub query_path: Option<&'a Path>,
    pub query_text: Option<&'a str>,
    pub source_path: Option<&'a Path>,
    pub source_text: Option<&'a str>,
    pub lang: Option<&'a str>,
    pub entry: Option<&'a str>,
    pub color: bool,
}

/// Prepared query ready for execution.
pub struct PreparedQuery {
    pub module: Module,
    pub entrypoint: Entrypoint,
    pub tree: tree_sitter::Tree,
    pub source_code: String,
}

/// Load, parse, analyze, link, and emit a query.
pub fn prepare_query(input: QueryInput) -> Result<PreparedQuery, CliError> {
    let loaded = load_query_source(input.query_path, input.query_text)?;

    validate(
        input.source_text,
        input.source_path,
        input.lang,
        loaded.shebang.lang.as_deref(),
    )?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    let source_code = load_source(input.source_text, input.source_path, input.query_path)?;
    let lang = resolve_lang(
        input.lang,
        loaded.shebang.lang.as_deref(),
        input.source_path,
    )?;

    let query = QueryBuilder::new(loaded.sources)
        .parse()
        .map_err(|e| CliError::fatal(e.to_string()))?
        .analyze()
        .link(lang.grammar());

    if !query.is_valid() {
        eprint!(
            "{}",
            query
                .diagnostics()
                .render_colored(query.source_map(), input.color)
        );
        return Err(CliError::FatalRendered);
    }

    let bytecode = emit(&query).map_err(|e| CliError::fatal(e.to_string()))?;
    let module = Module::load(&bytecode).expect("module load failed");

    let entry = input.entry.or(loaded.shebang.entry.as_deref());
    let entrypoint = resolve_entrypoint(&module, entry)?;
    let tree = lang.parse(&source_code);

    Ok(PreparedQuery {
        module,
        entrypoint,
        tree,
        source_code,
    })
}
