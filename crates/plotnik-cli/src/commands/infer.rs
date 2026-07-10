use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use plotnik_lib::{TypeScriptCodegenConfig, TypeScriptVoidType};

use super::compile::compile_query;
use super::lang_resolver::require_lang;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult};

pub struct InferArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub format: String,
    pub verbose_nodes: bool,
    pub no_node_type: bool,
    pub export: bool,
    pub output: Option<PathBuf>,
    pub color: bool,
    pub void_type: Option<String>,
}

pub fn run(args: InferArgs) -> CliResult {
    let fmt = args.format.to_lowercase();
    if fmt != "typescript" && fmt != "ts" {
        return Err(CliError::fatal("--format must be 'typescript' or 'ts'"));
    }

    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    let lang = require_lang(
        args.lang.as_deref(),
        loaded.shebang.lang.as_deref(),
        "infer",
    )?;

    let compiled = compile_query(loaded.sources, lang, args.color)?;

    let void_type = match args.void_type.as_deref() {
        Some("null") => TypeScriptVoidType::Null,
        _ => TypeScriptVoidType::Undefined,
    };
    // Only use colors when outputting to stdout (not to file)
    let use_colors = args.color && args.output.is_none();
    let config = TypeScriptCodegenConfig::new()
        .export(args.export)
        .emit_node_interface(!args.no_node_type)
        .verbose_nodes(args.verbose_nodes)
        .void_type(void_type)
        .colored(use_colors);
    let emission = compiled
        .emit_types(config)
        .map_err(|error| CliError::fatal(error.to_string()))?;
    let has_errors = emission.diagnostics().has_errors();
    if !emission.diagnostics().is_empty() {
        eprint!(
            "{}",
            emission
                .diagnostics()
                .render_colored(compiled.source_map(), args.color)
        );
    }
    if has_errors {
        return Err(CliError::No);
    }
    let output = emission
        .into_artifact()
        .expect("valid query emits TypeScript types")
        .into_parts()
        .0;

    if let Some(ref path) = args.output {
        fs::write(path, &output)
            .map_err(|e| CliError::fatal(format!("failed to write '{}': {}", path.display(), e)))?;
        let type_count = count_types(&output);
        eprintln!("Wrote {} types to {}", type_count, path.display());
    } else {
        io::stdout()
            .write_all(output.as_bytes())
            .expect("failed to write inferred types to stdout");
    }

    Ok(())
}

fn count_types(output: &str) -> usize {
    output
        .lines()
        .filter(|line| {
            line.starts_with("export type ")
                || line.starts_with("type ")
                || line.starts_with("export interface ")
                || line.starts_with("interface ")
        })
        .count()
}
