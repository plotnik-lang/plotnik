use std::path::PathBuf;

use plotnik_compiler::QueryBuilder;

use super::language::LanguageOptions;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult, write_stderr};

pub struct CheckArgs {
    pub query_path: PathBuf,
    pub language: LanguageOptions,
    pub strict: bool,
    pub color: bool,
}

pub fn run(args: CheckArgs) -> CliResult {
    let sources = load_query(&args.query_path)?.into_collection();
    let artifacts = args.language.load(None)?;
    let checked = QueryBuilder::from_sources(sources)
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
