use std::fs;
use std::path::PathBuf;

use plotnik_lib::{CodegenProvenance, RustCodegenConfig};

use clap::ValueEnum;

use super::compile::compile_query;
use super::grammar;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult, write_stderr, write_stdout, writeln_stderr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum GenerateTarget {
    Rust,
}

pub struct GenerateArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub grammar: PathBuf,
    pub target: GenerateTarget,
    pub output: Option<PathBuf>,
    pub debug: bool,
    pub color: bool,
}

pub fn run(args: GenerateArgs) -> CliResult {
    let output = generate(&args)?;
    if let Some(path) = &args.output {
        fs::write(path, output).map_err(|error| {
            CliError::fatal(format!("failed to write '{}': {error}", path.display()))
        })?;
        writeln_stderr(format_args!("Wrote Rust matcher to {}", path.display()))?;
        return Ok(());
    }

    write_stdout(format_args!("{output}"))?;
    Ok(())
}

pub(crate) fn generate(args: &GenerateArgs) -> Result<String, CliError> {
    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;
    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    let grammar = grammar::load(&args.grammar)?;
    let compiled = compile_query(loaded.sources, &grammar, args.color)?;

    match args.target {
        GenerateTarget::Rust => {
            let emission = compiled
                .emit(
                    RustCodegenConfig::new()
                        .debug(args.debug)
                        .provenance(CodegenProvenance::Full),
                )
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
            Ok(emission
                .into_artifact()
                .expect("valid query emits a Rust module")
                .into_source())
        }
    }
}
