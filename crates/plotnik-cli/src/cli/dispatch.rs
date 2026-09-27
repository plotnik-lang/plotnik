//! Extract compiler command options from clap matches.

use std::path::PathBuf;

use clap::ArgMatches;

use super::ColorChoice;
use crate::commands::check::CheckArgs;
use crate::commands::dump::DumpArgs;
use crate::commands::generate::{GenerateArgs, GenerateTarget};
use crate::commands::infer::InferArgs;

fn grammar_path(m: &ArgMatches) -> PathBuf {
    m.get_one::<PathBuf>("grammar")
        .cloned()
        .expect("clap requires --grammar")
}

pub struct CheckOpts {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub grammar: PathBuf,
    pub strict: bool,
    pub json: bool,
    pub color: ColorChoice,
}

impl CheckOpts {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            query_path: m.get_one::<PathBuf>("query_path").cloned(),
            query_text: m.get_one::<String>("query_text").cloned(),
            grammar: grammar_path(m),
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
            grammar: p.grammar,
            strict: p.strict,
            json: p.json,
            color: p.color.should_colorize(),
        }
    }
}

pub struct DumpOpts {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub grammar: PathBuf,
    pub color: ColorChoice,
}

impl DumpOpts {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            query_path: m.get_one::<PathBuf>("query_path").cloned(),
            query_text: m.get_one::<String>("query_text").cloned(),
            grammar: grammar_path(m),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<DumpOpts> for DumpArgs {
    fn from(p: DumpOpts) -> Self {
        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            grammar: p.grammar,
            color: p.color.should_colorize(),
        }
    }
}

pub struct InferOpts {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub grammar: PathBuf,
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
            grammar: grammar_path(m),
            format: m
                .get_one::<String>("format")
                .expect("clap supplies the format default")
                .clone(),
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
            grammar: p.grammar,
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
    pub grammar: PathBuf,
    pub target: GenerateTarget,
    pub output: Option<PathBuf>,
    pub debug: bool,
    pub color: ColorChoice,
}

impl GenerateOpts {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            query_path: m.get_one::<PathBuf>("query_path").cloned(),
            query_text: m.get_one::<String>("query_text").cloned(),
            grammar: grammar_path(m),
            target: m
                .get_one::<GenerateTarget>("target")
                .copied()
                .expect("clap guarantees --target is present"),
            output: m.get_one::<PathBuf>("output").cloned(),
            debug: m.get_flag("debug"),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<GenerateOpts> for GenerateArgs {
    fn from(p: GenerateOpts) -> Self {
        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            grammar: p.grammar,
            target: p.target,
            output: p.output,
            debug: p.debug,
            color: p.color.should_colorize(),
        }
    }
}
