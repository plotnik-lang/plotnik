use std::fs;
use std::path::PathBuf;

use plotnik_lib::{TypeScriptCodegenConfig, TypeScriptMatchOnlyType};

use super::compile::compile_query;
use super::lang_resolver::require_lang;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult, write_stderr, write_stdout, writeln_stderr};

pub struct InferArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub format: String,
    pub include_points: bool,
    pub no_node_type: bool,
    pub export: bool,
    pub output: Option<PathBuf>,
    pub color: bool,
    pub match_only_type: Option<String>,
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

    let match_only_type = match args.match_only_type.as_deref() {
        Some("null") => TypeScriptMatchOnlyType::Null,
        _ => TypeScriptMatchOnlyType::Undefined,
    };
    // Only use colors when outputting to stdout (not to file)
    let use_colors = args.color && args.output.is_none();
    let config = TypeScriptCodegenConfig::new()
        .export(args.export)
        .emit_node_interface(!args.no_node_type)
        .include_points(args.include_points)
        .match_only_type(match_only_type)
        .colored(use_colors);
    let emission = compiled
        .emit_types(config)
        .map_err(|error| CliError::fatal(error.to_string()))?;
    let has_errors = emission.diagnostics().has_errors();
    if !emission.diagnostics().is_empty() {
        write_stderr(format_args!(
            "{}",
            emission
                .diagnostics()
                .render_colored(compiled.source_map(), args.color)
        ))?;
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
        writeln_stderr(format_args!(
            "Wrote {} types to {}",
            type_count,
            path.display()
        ))?;
    } else {
        write_stdout(format_args!("{output}"))?;
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
