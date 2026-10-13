use std::path::PathBuf;

use plotnik_compiler::QueryBuilder;

use super::language::LanguageOptions;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult, write_stderr};

pub struct CheckArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub language: LanguageOptions,
    pub strict: bool,
    pub color: bool,
}

pub fn run(args: CheckArgs) -> CliResult {
    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    let artifacts = args.language.load_query(&loaded, None, args.color)?;
    let checked = QueryBuilder::new(loaded.sources)
        .with_strict_lints(args.strict)
        .compile(&artifacts.grammar)
        .map_err(|e| CliError::fatal(e.to_string()))?;

    let diagnostics = checked.diagnostics();
    let source_map = checked.source_map();
    let valid = if args.strict {
        !diagnostics.has_errors() && !diagnostics.has_warnings()
    } else {
        !diagnostics.has_errors()
    };

    if !valid || diagnostics.has_warnings() {
        write_stderr(format_args!(
            "{}",
            diagnostics.render_colored(source_map, args.color)
        ))?;
    }

    if !valid {
        return Err(CliError::No);
    }

    Ok(())
}
