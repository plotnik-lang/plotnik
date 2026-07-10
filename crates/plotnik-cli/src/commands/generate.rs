use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use plotnik_lib::grammar::{Grammar, raw::RawGrammar};
use plotnik_lib::{CodegenProvenance, GrammarIdentity, RustCodegenConfig};

use clap::ValueEnum;

use super::compile::{compile_query, compile_query_with_grammar};
use super::lang_resolver::require_lang;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult};
use crate::language_registry;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum GenerateTarget {
    Rust,
}

pub struct GenerateArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub grammar: Option<PathBuf>,
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
        eprintln!("Wrote Rust matcher to {}", path.display());
        return Ok(());
    }

    io::stdout()
        .write_all(output.as_bytes())
        .map_err(|error| CliError::fatal(format!("failed to write generated matcher: {error}")))?;
    Ok(())
}

pub(crate) fn generate(args: &GenerateArgs) -> Result<String, CliError> {
    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;
    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    let compiled = if let Some(path) = &args.grammar {
        let external = load_external_grammar(path)?;
        validate_declared_language(loaded.shebang.lang.as_deref(), &external.identity)?;
        compile_query_with_grammar(loaded.sources, &external.grammar, args.color)
            .map_err(generate_compile_error)?
    } else {
        let lang = require_lang(
            args.lang.as_deref(),
            loaded.shebang.lang.as_deref(),
            "generate",
        )?;
        compile_query(loaded.sources, lang, args.color).map_err(generate_compile_error)?
    };

    match args.target {
        GenerateTarget::Rust => {
            let emission = compiled
                .emit(RustCodegenConfig::new().provenance(CodegenProvenance::Full))
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
            Ok(emission
                .into_artifact()
                .expect("valid query emits a Rust module")
                .into_source())
        }
    }
}

fn generate_compile_error(error: CliError) -> CliError {
    match error {
        CliError::FatalRendered => CliError::No,
        error => error,
    }
}

struct ExternalGrammar {
    grammar: Grammar,
    identity: GrammarIdentity,
}

fn load_external_grammar(path: &Path) -> Result<ExternalGrammar, CliError> {
    let bytes = fs::read(path).map_err(|error| {
        CliError::fatal(format!(
            "failed to read grammar '{}': {error}",
            path.display()
        ))
    })?;
    let json = std::str::from_utf8(&bytes).map_err(|error| {
        CliError::fatal(format!(
            "grammar '{}' is not valid UTF-8: {error}",
            path.display()
        ))
    })?;
    let raw = RawGrammar::from_json(json).map_err(|error| {
        CliError::fatal(format!(
            "failed to parse grammar '{}': {error:?}",
            path.display()
        ))
    })?;
    let identity =
        GrammarIdentity::from_json_bytes(raw.name.clone(), &bytes, path.display().to_string());
    let grammar = Grammar::from_raw(&raw)
        .map_err(|error| {
            CliError::fatal(format!(
                "failed to load grammar metadata '{}': {error:?}",
                path.display()
            ))
        })?
        .with_identity(identity.clone());
    Ok(ExternalGrammar { grammar, identity })
}

fn validate_declared_language(
    declared: Option<&str>,
    identity: &GrammarIdentity,
) -> Result<(), CliError> {
    let Some(declared) = declared else {
        return Ok(());
    };
    let agrees = declared.eq_ignore_ascii_case(identity.name())
        || language_registry::from_name(declared)
            .is_some_and(|language| language.name() == identity.name());
    if agrees {
        return Ok(());
    }
    Err(CliError::fatal(format!(
        "query shebang declares language '{declared}', but --grammar contains '{}'",
        identity.name()
    )))
}
