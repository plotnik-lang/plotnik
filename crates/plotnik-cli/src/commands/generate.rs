use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use plotnik_lib::grammar::{Grammar, raw::RawGrammar};
use plotnik_lib::{GrammarIdentity, MatcherConfig};

use super::compile::{compile_query, compile_query_with_grammar};
use super::lang_resolver::require_lang;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult};
use crate::language_registry;

pub struct GenerateArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub grammar: Option<PathBuf>,
    pub target: String,
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
    if args.target != "rust" {
        return Err(CliError::fatal("--target must be 'rust'"));
    }
    if args.grammar.is_some() && args.lang.is_some() {
        return Err(CliError::fatal(
            "--grammar cannot be combined with -l/--lang",
        ));
    }

    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;
    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    let (compiled, identity) = if let Some(path) = &args.grammar {
        let external = load_external_grammar(path)?;
        validate_declared_language(loaded.shebang.lang.as_deref(), &external.identity)?;
        let compiled =
            compile_query_with_grammar(loaded.sources, &external.grammar, args.color, false)
                .map_err(generate_compile_error)?;
        (compiled, external.identity)
    } else {
        let lang = require_lang(
            args.lang.as_deref(),
            loaded.shebang.lang.as_deref(),
            "generate",
        )?;
        let identity = GrammarIdentity::from_json_bytes(
            lang.raw().name.clone(),
            lang.grammar_json().as_bytes(),
            lang.source(),
        );
        let compiled = compile_query(loaded.sources, lang, args.color, false)
            .map_err(generate_compile_error)?;
        (compiled, identity)
    };

    Ok(compiled
        .to_rust_matcher(MatcherConfig::new().grammar_identity(identity))
        .expect("successful full-pipeline compilation produces a matcher"))
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
    let grammar = Grammar::from_raw(&raw).map_err(|error| {
        CliError::fatal(format!(
            "failed to load grammar metadata '{}': {error:?}",
            path.display()
        ))
    })?;
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
