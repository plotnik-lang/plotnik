use std::path::PathBuf;

use plotnik_compiler::QueryBuilder;

use super::compile::report_diagnostics;
use super::language::LanguageOptions;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult};

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
    report_diagnostics(diagnostics, checked.source_map(), args.color)?;

    if args.strict && diagnostics.has_warnings() {
        return Err(CliError::No);
    }

    Ok(())
}
