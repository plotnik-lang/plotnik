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
use crate::commands::check::CheckArgs;
use crate::commands::dump::DumpArgs;
use crate::commands::generate::{GenerateArgs, GenerateTarget};
use crate::commands::infer::InferArgs;
use crate::commands::inspect::InspectArgs;
use crate::commands::run::RunArgs;
use crate::commands::trace::TraceArgs;
use crate::commands::tree::{QueryView, TreeArgs};

pub struct TreeOpts {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub query_view: QueryView,
    pub include_anonymous: bool,
    pub json: bool,
    pub color: ColorChoice,
}

impl TreeOpts {
    pub fn from_matches(m: &ArgMatches) -> Self {
        let query_path = m.get_one::<PathBuf>("query_path").cloned();
        let query_text = m.get_one::<String>("query_text").cloned();
        let source_path = m.get_one::<PathBuf>("source_path").cloned();

        let (query_path, source_path) =
            shift_positional_to_source(query_text.is_some(), query_path, source_path);

        let (query_path, source_path) =
            classify_tree_positional(query_path, source_path, query_text.is_some());

        Self {
            query_path,
            query_text,
            source_path,
            source_text: m.get_one::<String>("source_text").cloned(),
            lang: m.get_one::<String>("lang").cloned(),
            query_view: match m.get_one::<String>("query_view").map(String::as_str) {
                Some("cst") => QueryView::Cst,
                _ => QueryView::Ast,
            },
            include_anonymous: m.get_flag("include_anonymous"),
            json: m.get_flag("json"),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<TreeOpts> for TreeArgs {
    fn from(p: TreeOpts) -> Self {
        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            source_path: p.source_path,
            source_text: p.source_text,
            lang: p.lang,
            query_view: p.query_view,
            include_anonymous: p.include_anonymous,
            json: p.json,
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
    // Note: source_path, source_text, entry, compact, include_points, verbose,
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
    pub include_points: bool,
    pub no_node_type: bool,
    pub no_export: bool,
    pub match_only_type: Option<String>,
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
            include_points: m.get_flag("include_points"),
            no_node_type: m.get_flag("no_node_type"),
            no_export: m.get_flag("no_export"),
            match_only_type: m.get_one::<String>("match_only_type").cloned(),
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
            include_points: p.include_points,
            no_node_type: p.no_node_type,
            export: !p.no_export,
            output: p.output,
            color: p.color.should_colorize(),
            match_only_type: p.match_only_type,
        }
    }
}

pub struct GenerateOpts {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub grammar: Option<PathBuf>,
    pub target: GenerateTarget,
    pub output: Option<PathBuf>,
    pub color: ColorChoice,
}

impl GenerateOpts {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            query_path: m.get_one::<PathBuf>("query_path").cloned(),
            query_text: m.get_one::<String>("query_text").cloned(),
            lang: m.get_one::<String>("lang").cloned(),
            grammar: m.get_one::<PathBuf>("grammar").cloned(),
            target: m
                .get_one::<GenerateTarget>("target")
                .copied()
                .expect("clap guarantees --target is present"),
            output: m.get_one::<PathBuf>("output").cloned(),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<GenerateOpts> for GenerateArgs {
    fn from(options: GenerateOpts) -> Self {
        Self {
            query_path: options.query_path,
            query_text: options.query_text,
            lang: options.lang,
            grammar: options.grammar,
            target: options.target,
            output: options.output,
            color: options.color.should_colorize(),
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
    // Note: include_points, verbose, no_result are hidden unified flags,
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
    // Note: compact, include_points are parsed but not extracted (unified flags)
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

pub struct InspectOpts {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub entry: Option<String>,
    pub verbose: u8,
    pub limits: RuntimeLimitSpec,
    pub json: bool,
    pub color: ColorChoice,
}

impl InspectOpts {
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
            limits: resolve_limit_spec(m),
            json: m.get_flag("json"),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<InspectOpts> for InspectArgs {
    fn from(p: InspectOpts) -> Self {
        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            source_path: p.source_path,
            source_text: p.source_text,
            lang: p.lang,
            entry: p.entry,
            limits: p.limits,
            json: p.json,
            trace: p.verbose > 0,
            color: p.color.should_colorize(),
        }
    }
}

pub struct LangDumpOpts {
    pub lang: String,
    pub legend: bool,
    pub json: bool,
    pub width: Option<usize>,
}

impl LangDumpOpts {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            lang: m
                .get_one::<String>("lang")
                .cloned()
                .expect("clap guarantees `lang` is present"),
            legend: !m.get_flag("no-legend"),
            json: m.get_flag("json"),
            width: m.get_one::<usize>("width").copied(),
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

// `tree` takes a single positional for either role; disambiguate by extension.
fn classify_tree_positional(
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
