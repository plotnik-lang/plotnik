//! Extract compiler command options from clap matches.

use std::path::PathBuf;

use clap::ArgMatches;

use super::ColorChoice;
use crate::commands::check::CheckArgs;
use crate::commands::dump::DumpArgs;
use crate::commands::generate::{GenerateArgs, GenerateTarget};
use crate::commands::infer::InferArgs;
use crate::commands::language::LanguageOptions;

pub struct CheckOpts {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub language: LanguageOptions,
    pub strict: bool,
    pub color: ColorChoice,
}

impl CheckOpts {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            query_path: m.get_one::<PathBuf>("query_path").cloned(),
            query_text: m.get_one::<String>("query_text").cloned(),
            language: LanguageOptions::from_matches(m),
            strict: m.get_flag("strict"),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<CheckOpts> for CheckArgs {
    fn from(p: CheckOpts) -> Self {
        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            language: p.language,
            strict: p.strict,
            color: p.color.should_colorize(),
        }
    }
}

pub struct DumpOpts {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub language: LanguageOptions,
    pub color: ColorChoice,
}

impl DumpOpts {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            query_path: m.get_one::<PathBuf>("query_path").cloned(),
            query_text: m.get_one::<String>("query_text").cloned(),
            language: LanguageOptions::from_matches(m),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<DumpOpts> for DumpArgs {
    fn from(p: DumpOpts) -> Self {
        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            language: p.language,
            color: p.color.should_colorize(),
        }
    }
}

pub struct InferOpts {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub language: LanguageOptions,
    pub format: String,
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
            language: LanguageOptions::from_matches(m),
            format: m
                .get_one::<String>("format")
                .expect("clap supplies the format default")
                .clone(),
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
            language: p.language,
            format: p.format,
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
    pub language: LanguageOptions,
    pub target: GenerateTarget,
    pub output: Option<PathBuf>,
    pub color: ColorChoice,
}

impl GenerateOpts {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            query_path: m.get_one::<PathBuf>("query_path").cloned(),
            query_text: m.get_one::<String>("query_text").cloned(),
            language: LanguageOptions::from_matches(m),
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
    fn from(p: GenerateOpts) -> Self {
        Self {
            query_path: p.query_path,
            query_text: p.query_text,
            language: p.language,
            target: p.target,
            output: p.output,
            color: p.color.should_colorize(),
        }
    }
}
