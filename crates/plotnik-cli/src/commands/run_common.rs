//! Shared logic for exec and trace commands.

use std::fs;
use std::io::{self, Read};
use std::path::Path;

use arborium_tree_sitter as tree_sitter;
use plotnik_langs::Lang;
use plotnik_lib::bytecode::{Entrypoint, Module};
use plotnik_lib::emit::emit_linked;
use plotnik_lib::QueryBuilder;

use super::lang_resolver::{resolve_lang_required, suggest_language};
use super::query_loader::load_query_source;

/// Load source code from file, stdin, or inline text.
pub fn load_source(
    source_text: Option<&str>,
    source_path: Option<&Path>,
    query_path: Option<&Path>,
) -> String {
    if let Some(text) = source_text {
        return text.to_owned();
    }
    if let Some(path) = source_path {
        if path.as_os_str() == "-" {
            if query_path.map(|p| p.as_os_str() == "-").unwrap_or(false) {
                eprintln!("error: query and source cannot both be from stdin");
                std::process::exit(1);
            }
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .expect("failed to read stdin");
            return buf;
        }
        return fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("error: failed to read '{}': {}", path.display(), e);
            std::process::exit(1);
        });
    }
    unreachable!("validation ensures source input exists")
}

/// Resolve source language from --lang flag or file extension.
pub fn resolve_lang(lang_name: Option<&str>, source_path: Option<&Path>) -> Lang {
    if let Some(name) = lang_name {
        return resolve_lang_required(name).unwrap_or_else(|msg| {
            eprintln!("error: {}", msg);
            if let Some(suggestion) = suggest_language(name) {
                eprintln!();
                eprintln!("Did you mean '{}'?", suggestion);
            }
            eprintln!();
            eprintln!("Run 'plotnik langs' for the full list.");
            std::process::exit(1);
        });
    }

    if let Some(path) = source_path
        && path.as_os_str() != "-"
        && let Some(ext) = path.extension().and_then(|e| e.to_str())
    {
        if let Some(lang) = plotnik_langs::from_ext(ext) {
            return lang;
        }
        eprintln!(
            "error: cannot infer language from extension '.{}', use --lang",
            ext
        );
        std::process::exit(1);
    }

    eprintln!("error: --lang is required (cannot infer from input)");
    std::process::exit(1)
}

/// Resolve entrypoint by name or use the single available one.
pub fn resolve_entrypoint(module: &Module, name: Option<&str>) -> Entrypoint {
    let entries = module.entrypoints();
    let strings = module.strings();

    match name {
        Some(name) => entries.find_by_name(name, &strings).unwrap_or_else(|| {
            eprintln!("error: invalid entrypoint: {}", name);
            std::process::exit(1);
        }),
        None => {
            if entries.len() == 1 {
                entries.get(0)
            } else if entries.is_empty() {
                eprintln!("error: no entrypoints in module");
                std::process::exit(1);
            } else {
                eprintln!("error: multiple entrypoints, specify one with --entry");
                std::process::exit(1);
            }
        }
    }
}

/// Validate common arguments.
pub fn validate(
    has_query: bool,
    has_source: bool,
    source_is_inline: bool,
    has_lang: bool,
) -> Result<(), &'static str> {
    if !has_query {
        return Err("query is required: use positional argument, -q/--query, or --query-file");
    }
    if !has_source {
        return Err("source is required: use positional argument, -s/--source-file, or --source");
    }
    if source_is_inline && !has_lang {
        return Err("--lang is required when using --source");
    }
    Ok(())
}

/// Build trivia type list from module.
pub fn build_trivia_types(module: &Module) -> Vec<u16> {
    let trivia_view = module.trivia();
    (0..trivia_view.len())
        .map(|i| trivia_view.get(i).node_type)
        .collect()
}

/// Common input parameters for exec/trace commands.
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
    pub trivia_types: Vec<u16>,
    pub source_code: String,
}

/// Load, parse, analyze, link, and emit a query.
/// Exits on any error.
pub fn prepare_query(input: QueryInput) -> PreparedQuery {
    if let Err(msg) = validate(
        input.query_text.is_some() || input.query_path.is_some(),
        input.source_text.is_some() || input.source_path.is_some(),
        input.source_text.is_some(),
        input.lang.is_some(),
    ) {
        eprintln!("error: {}", msg);
        std::process::exit(1);
    }

    let source_map = match load_query_source(input.query_path, input.query_text) {
        Ok(map) => map,
        Err(msg) => {
            eprintln!("error: {}", msg);
            std::process::exit(1);
        }
    };

    if source_map.is_empty() {
        eprintln!("error: query cannot be empty");
        std::process::exit(1);
    }

    let source_code = load_source(input.source_text, input.source_path, input.query_path);
    let lang = resolve_lang(input.lang, input.source_path);

    let query = match QueryBuilder::new(source_map).parse() {
        Ok(parsed) => parsed.analyze().link(&lang),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };

    if !query.is_valid() {
        eprint!(
            "{}",
            query
                .diagnostics()
                .render_colored(query.source_map(), input.color)
        );
        std::process::exit(1);
    }

    let bytecode = emit_linked(&query).expect("emit failed");
    let module = Module::from_bytes(bytecode).expect("module load failed");

    let entrypoint = resolve_entrypoint(&module, input.entry);
    let tree = lang.parse(&source_code);
    let trivia_types = build_trivia_types(&module);

    PreparedQuery {
        module,
        entrypoint,
        tree,
        trivia_types,
        source_code,
    }
}
