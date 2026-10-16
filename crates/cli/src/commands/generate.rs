use std::fs;
use std::path::PathBuf;

use plotnik_compiler::{CodegenProvenance, RustCodegenConfig};

use clap::ValueEnum;

use super::compile::{compile_query, report_diagnostics};
use super::language::LanguageOptions;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult, write_stdout, writeln_stderr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum GenerateTarget {
    Rust,
}

pub struct GenerateArgs {
    pub query_path: PathBuf,
    pub language: LanguageOptions,
    pub target: GenerateTarget,
    pub output: Option<PathBuf>,
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
    let sources = load_query(&args.query_path)?.into_collection();
    let artifacts = args.language.load(None)?;
    let compiled = compile_query(sources, &artifacts.grammar, args.color)?;

    match args.target {
        GenerateTarget::Rust => {
            let emission = compiled
                .emit(RustCodegenConfig::new().provenance(CodegenProvenance::Full))
                .map_err(|error| CliError::fatal(error.to_string()))?;
            report_diagnostics(emission.diagnostics(), compiled.source_map(), args.color)?;
            Ok(emission
                .into_artifact()
                .expect("valid query emits a Rust module")
                .into_source())
        }
    }
}
