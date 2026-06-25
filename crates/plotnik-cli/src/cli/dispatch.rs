//! Dispatch logic: extract params from ArgMatches and convert to command args.
//!
//! This module contains:
//! - `*Opts` structs that mirror command `*Args` but are populated from clap
//! - `from_matches()` extractors that pull relevant fields (ignoring hidden ones)
//! - `Into<*Args>` impls to bridge dispatch → command handlers
//! - Positional shifting logic for exec/trace (`-q` shifts first positional to source)

use std::path::PathBuf;

use clap::ArgMatches;
use plotnik_lib::RuntimeLimitSpec;

use super::ColorChoice;
use super::limits::resolve_limit_spec;
use crate::commands::ast::AstArgs;
use crate::commands::check::CheckArgs;
use crate::commands::dump::DumpArgs;
use crate::commands::infer::InferArgs;
use crate::commands::run::RunArgs;
use crate::commands::trace::TraceArgs;

pub struct AstOpts {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub raw: bool,
    pub color: ColorChoice,
}

impl AstOpts {
    pub fn from_matches(m: &ArgMatches) -> Self {
        let query_path = m.get_one::<PathBuf>("query_path").cloned();
        let query_text = m.get_one::<String>("query_text").cloned();
        let source_path = m.get_one::<PathBuf>("source_path").cloned();

        let (query_path, source_path) =
            shift_positional_to_source(query_text.is_some(), query_path, source_path);

        let (query_path, source_path) =
            classify_ast_positional(query_path, source_path, query_text.is_some());

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

impl From<AstOpts> for AstArgs {
    fn from(p: AstOpts) -> Self {
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

pub struct CheckOpts {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub strict: bool,
    pub json: bool,
    pub color: ColorChoice,
}

impl CheckOpts {
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

impl From<CheckOpts> for CheckArgs {
    fn from(p: CheckOpts) -> Self {
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

pub struct DumpOpts {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub color: ColorChoice,
    // Note: source_path, source_text, entry, compact, verbose_nodes, verbose,
    // no_result, and the runtime-limit flags are parsed but not extracted.
}

impl DumpOpts {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            query_path: m.get_one::<PathBuf>("query_path").cloned(),
            query_text: m.get_one::<String>("query_text").cloned(),
            lang: m.get_one::<String>("lang").cloned(),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<DumpOpts> for DumpArgs {
    fn from(p: DumpOpts) -> Self {
        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            lang: p.lang,
            color: p.color.should_colorize(),
        }
    }
}

pub struct InferOpts {
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

impl InferOpts {
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

impl From<InferOpts> for InferArgs {
    fn from(p: InferOpts) -> Self {
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

pub struct RunOpts {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub compact: bool,
    pub entry: Option<String>,
    pub limits: RuntimeLimitSpec,
    pub json: bool,
    pub color: ColorChoice,
    // Note: verbose_nodes, verbose, no_result are hidden unified flags,
    // parsed but not extracted.
}

impl RunOpts {
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
            limits: resolve_limit_spec(m),
            json: m.get_flag("json"),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<RunOpts> for RunArgs {
    fn from(p: RunOpts) -> Self {
        let pretty = !p.compact && std::io::IsTerminal::is_terminal(&std::io::stdout());

        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            source_path: p.source_path,
            source_text: p.source_text,
            lang: p.lang,
            pretty,
            entry: p.entry,
            limits: p.limits,
            json: p.json,
            color: p.color.should_colorize(),
        }
    }
}

pub struct TraceOpts {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub entry: Option<String>,
    pub verbose: u8,
    pub no_result: bool,
    pub limits: RuntimeLimitSpec,
    pub json: bool,
    pub color: ColorChoice,
    // Note: compact, verbose_nodes are parsed but not extracted (unified flags)
}

impl TraceOpts {
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
            limits: resolve_limit_spec(m),
            json: m.get_flag("json"),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<TraceOpts> for TraceArgs {
    fn from(p: TraceOpts) -> Self {
        use plotnik_lib::Verbosity;

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
            limits: p.limits,
            json: p.json,
            color: p.color.should_colorize(),
        }
    }
}

pub struct LangDumpOpts {
    pub lang: String,
}

impl LangDumpOpts {
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
fn classify_ast_positional(
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
