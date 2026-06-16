//! Dispatch logic: extract params from ArgMatches and convert to command args.
//!
//! This module contains:
//! - `*Params` structs that mirror command `*Args` but are populated from clap
//! - `from_matches()` extractors that pull relevant fields (ignoring hidden ones)
//! - `Into<*Args>` impls to bridge dispatch → command handlers
//! - Positional shifting logic for exec/trace (`-q` shifts first positional to source)

use std::path::PathBuf;

use clap::ArgMatches;

use super::ColorChoice;
use crate::commands::ast::AstArgs;
use crate::commands::check::CheckArgs;
use crate::commands::dump::DumpArgs;
use crate::commands::infer::InferArgs;
use crate::commands::run::RunArgs;
use crate::commands::trace::TraceArgs;

pub struct AstParams {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub raw: bool,
    pub color: ColorChoice,
}

impl AstParams {
    pub fn from_matches(m: &ArgMatches) -> Self {
        let query_path = m.get_one::<PathBuf>("query_path").cloned();
        let query_text = m.get_one::<String>("query_text").cloned();
        let source_path = m.get_one::<PathBuf>("source_path").cloned();

        let (query_path, source_path) =
            shift_positional_to_source(query_text.is_some(), query_path, source_path);

        let (query_path, source_path) =
            detect_file_type_by_extension(query_path, source_path, query_text.is_some());

        Self {
            query_path,
            query_text,
            source_path,
            source_text: m.get_one::<String>("source_text").cloned(),
            lang: m.get_one::<String>("lang").cloned(),
            raw: m.get_flag("raw"),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<AstParams> for AstArgs {
    fn from(p: AstParams) -> Self {
        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            source_path: p.source_path,
            source_text: p.source_text,
            lang: p.lang,
            raw: p.raw,
            color: p.color.should_colorize(),
        }
    }
}

pub struct CheckParams {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub strict: bool,
    pub json: bool,
    pub color: ColorChoice,
}

impl CheckParams {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            query_path: m.get_one::<PathBuf>("query_path").cloned(),
            query_text: m.get_one::<String>("query_text").cloned(),
            lang: m.get_one::<String>("lang").cloned(),
            strict: m.get_flag("strict"),
            json: m.get_flag("json"),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<CheckParams> for CheckArgs {
    fn from(p: CheckParams) -> Self {
        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            lang: p.lang,
            strict: p.strict,
            json: p.json,
            color: p.color.should_colorize(),
        }
    }
}

pub struct DumpParams {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub color: ColorChoice,
    // Note: source_path, source_text, entry, compact, verbose_nodes,
    // verbose, no_result, fuel are parsed but not extracted (unified flags)
}

impl DumpParams {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            query_path: m.get_one::<PathBuf>("query_path").cloned(),
            query_text: m.get_one::<String>("query_text").cloned(),
            lang: m.get_one::<String>("lang").cloned(),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<DumpParams> for DumpArgs {
    fn from(p: DumpParams) -> Self {
        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            lang: p.lang,
            color: p.color.should_colorize(),
        }
    }
}

pub struct InferParams {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub format: String,
    pub verbose_nodes: bool,
    pub no_node_type: bool,
    pub no_export: bool,
    pub void_type: Option<String>,
    pub output: Option<PathBuf>,
    pub color: ColorChoice,
}

impl InferParams {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            query_path: m.get_one::<PathBuf>("query_path").cloned(),
            query_text: m.get_one::<String>("query_text").cloned(),
            lang: m.get_one::<String>("lang").cloned(),
            format: m
                .get_one::<String>("format")
                .cloned()
                .unwrap_or_else(|| "typescript".to_string()),
            verbose_nodes: m.get_flag("verbose_nodes"),
            no_node_type: m.get_flag("no_node_type"),
            no_export: m.get_flag("no_export"),
            void_type: m.get_one::<String>("void_type").cloned(),
            output: m.get_one::<PathBuf>("output").cloned(),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<InferParams> for InferArgs {
    fn from(p: InferParams) -> Self {
        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            lang: p.lang,
            format: p.format,
            verbose_nodes: p.verbose_nodes,
            no_node_type: p.no_node_type,
            export: !p.no_export,
            output: p.output,
            color: p.color.should_colorize(),
            void_type: p.void_type,
        }
    }
}

pub struct RunParams {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub compact: bool,
    pub entry: Option<String>,
    pub color: ColorChoice,
    // Note: verbose_nodes, verbose, no_result, fuel are hidden unified flags,
    // parsed but not extracted.
}

impl RunParams {
    pub fn from_matches(m: &ArgMatches) -> Self {
        let query_path = m.get_one::<PathBuf>("query_path").cloned();
        let query_text = m.get_one::<String>("query_text").cloned();
        let source_path = m.get_one::<PathBuf>("source_path").cloned();

        let (query_path, source_path) =
            shift_positional_to_source(query_text.is_some(), query_path, source_path);

        Self {
            query_path,
            query_text,
            source_path,
            source_text: m.get_one::<String>("source_text").cloned(),
            lang: m.get_one::<String>("lang").cloned(),
            compact: m.get_flag("compact"),
            entry: m.get_one::<String>("entry").cloned(),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<RunParams> for RunArgs {
    fn from(p: RunParams) -> Self {
        let pretty = !p.compact && std::io::IsTerminal::is_terminal(&std::io::stdout());

        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            source_path: p.source_path,
            source_text: p.source_text,
            lang: p.lang,
            pretty,
            entry: p.entry,
            color: p.color.should_colorize(),
        }
    }
}

pub struct TraceParams {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub entry: Option<String>,
    pub verbose: u8,
    pub no_result: bool,
    pub fuel: u32,
    pub color: ColorChoice,
    // Note: compact, verbose_nodes are parsed but not extracted (unified flags)
}

impl TraceParams {
    pub fn from_matches(m: &ArgMatches) -> Self {
        let query_path = m.get_one::<PathBuf>("query_path").cloned();
        let query_text = m.get_one::<String>("query_text").cloned();
        let source_path = m.get_one::<PathBuf>("source_path").cloned();

        let (query_path, source_path) =
            shift_positional_to_source(query_text.is_some(), query_path, source_path);

        Self {
            query_path,
            query_text,
            source_path,
            source_text: m.get_one::<String>("source_text").cloned(),
            lang: m.get_one::<String>("lang").cloned(),
            entry: m.get_one::<String>("entry").cloned(),
            verbose: m.get_count("verbose"),
            no_result: m.get_flag("no_result"),
            fuel: m.get_one::<u32>("fuel").copied().unwrap_or(1_000_000),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<TraceParams> for TraceArgs {
    fn from(p: TraceParams) -> Self {
        use plotnik_lib::engine::Verbosity;

        let verbosity = match p.verbose {
            0 => Verbosity::Default,
            1 => Verbosity::Verbose,
            _ => Verbosity::VeryVerbose,
        };

        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            source_path: p.source_path,
            source_text: p.source_text,
            lang: p.lang,
            entry: p.entry,
            verbosity,
            no_result: p.no_result,
            fuel: p.fuel,
            color: p.color.should_colorize(),
        }
    }
}

pub struct LangListParams;

impl LangListParams {
    pub fn from_matches(_m: &ArgMatches) -> Self {
        Self
    }
}

pub struct LangDumpParams {
    pub lang: String,
}

impl LangDumpParams {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            lang: m
                .get_one::<String>("lang")
                .cloned()
                .expect("clap guarantees `lang` is present"),
        }
    }
}

// `-q TEXT source.js` — clap assigns the positional to query_path, so shift it to source.
fn shift_positional_to_source(
    has_query_text: bool,
    query_path: Option<PathBuf>,
    source_path: Option<PathBuf>,
) -> (Option<PathBuf>, Option<PathBuf>) {
    if has_query_text && query_path.is_some() && source_path.is_none() {
        (None, query_path)
    } else {
        (query_path, source_path)
    }
}

// `ast` takes a single positional for either role; disambiguate by extension.
fn detect_file_type_by_extension(
    query_path: Option<PathBuf>,
    source_path: Option<PathBuf>,
    has_query_text: bool,
) -> (Option<PathBuf>, Option<PathBuf>) {
    if has_query_text || source_path.is_some() {
        return (query_path, source_path);
    }

    let Some(path) = query_path else {
        return (None, None);
    };

    if path.extension().is_some_and(|ext| ext == "ptk") {
        return (Some(path), None);
    }

    (None, Some(path))
}
