use std::path::PathBuf;

use plotnik_compiler::QueryBuilder;

use super::compile::{emit_module, report_diagnostics};
use super::language::LanguageOptions;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult, writeln_stderr};

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

    // Every query error caught here must be implemented at the diagnostics level.
    if let Err(error) = emit_module(&checked, args.color) {
        if matches!(error, CliError::No) {
            writeln_stderr(format_args!(
                "error: query validation missed a bytecode emission error"
            ))?;
        }

        return Err(error);
    }

    if args.strict && diagnostics.has_warnings() {
        return Err(CliError::No);
    }

    Ok(())
}
