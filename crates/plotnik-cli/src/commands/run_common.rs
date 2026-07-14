//! Shared logic for run and trace commands.

use std::fs;
use std::io::{self, Read};
use std::path::Path;

use plotnik_lib::bytecode::{EntryPoint, Module};
use plotnik_lib::text_utils::find_similar;

use super::compile::compile_query;
use super::lang_resolver::reconcile_lang;
use super::query_loader::load_query;
use crate::error::CliError;
use crate::language_registry::{self, Lang};

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

/// Reject an input supplied two ways at once. clap fills the query slot from
/// the first positional and the source slot from the second; when `-q`/`-s`
/// also supplies that role inline, the loaders below silently prefer the text
/// and drop the positional. The query+source commands (run/trace/inspect/tree)
/// refuse to guess. Query-only commands (check/dump/infer) do not call this:
/// they deliberately ignore an extra positional so a run-shaped command line
/// still works.
pub fn reject_ambiguous_inputs(
    query_text: Option<&str>,
    query_path: Option<&Path>,
    source_text: Option<&str>,
    source_path: Option<&Path>,
) -> Result<(), CliError> {
    if query_text.is_some() && query_path.is_some() {
        return Err(CliError::fatal(
            "query supplied twice: pass either -q/--query or a positional path, not both",
        ));
    }
    if source_text.is_some() && source_path.is_some() {
        return Err(CliError::fatal(
            "source supplied twice: pass either -s/--source or a positional path, not both",
        ));
    }
    Ok(())
}

/// Resolve the source language.
/// Priority: explicit `-l` (must agree with shebang) > shebang > source extension.
pub fn resolve_run_lang(
    explicit: Option<&str>,
    declared: Option<&str>,
    source_path: Option<&Path>,
) -> Result<&'static Lang, CliError> {
    if let Some(lang) = reconcile_lang(explicit, declared)? {
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

/// Resolve the selected entry point after defaulting has already happened.
pub fn resolve_entry_point(module: &Module, name: Option<&str>) -> Result<EntryPoint, CliError> {
    match name {
        Some(name) => module.entry_point(name).ok_or_else(|| {
            let names: Vec<&str> = module.entry_point_names().collect();
            let mut msg = format!("invalid entry point: {}", name);
            if let Some(similar) = find_similar(name, &names) {
                msg.push_str(&format!("\n\nDid you mean '{}'?", similar));
            }
            msg.push_str(&format!("\n\nAvailable entry points: {}", names.join(", ")));
            CliError::fatal(msg)
        }),
        None => Err(CliError::fatal("bytecode module exports no entry points")),
    }
}

fn require_source_input(
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
pub struct ExecRequest<'a> {
    pub query_path: Option<&'a Path>,
    pub query_text: Option<&'a str>,
    pub source_path: Option<&'a Path>,
    pub source_text: Option<&'a str>,
    pub lang: Option<&'a str>,
    pub entry: Option<&'a str>,
    pub color: bool,
    pub inspection: bool,
}

/// Prepared query ready for execution.
pub struct ExecPlan {
    pub module: Module,
    pub entry_point: EntryPoint,
    pub tree: tree_sitter::Tree,
    pub source_code: String,
}

pub fn plan_exec(input: ExecRequest) -> Result<ExecPlan, CliError> {
    reject_ambiguous_inputs(
        input.query_text,
        input.query_path,
        input.source_text,
        input.source_path,
    )?;

    let loaded = load_query(input.query_path, input.query_text)?;

    require_source_input(
        input.source_text,
        input.source_path,
        input.lang,
        loaded.shebang.lang.as_deref(),
    )?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    let source_code = load_source(input.source_text, input.source_path, input.query_path)?;
    let lang = resolve_run_lang(
        input.lang,
        loaded.shebang.lang.as_deref(),
        input.source_path,
    )?;

    let compiled = compile_query(loaded.sources, lang, input.color)?;
    let config = if input.inspection {
        plotnik_lib::BytecodeConfig::new().inspection(plotnik_lib::BytecodeInspection::Spans)
    } else {
        plotnik_lib::BytecodeConfig::new()
    };
    let module = super::compile::emit_module(&compiled, config, input.color)?;
    // Queries conventionally put the preferred selectable definition last.
    let default_entry = module.entry_point_names().last().map(str::to_owned);

    let entry = input
        .entry
        .map(str::to_owned)
        .or_else(|| loaded.shebang.entry.clone())
        .or(default_entry);
    let entry_point = resolve_entry_point(&module, entry.as_deref())?;
    let tree = lang.parse_source(&source_code);

    Ok(ExecPlan {
        module,
        entry_point,
        tree,
        source_code,
    })
}
