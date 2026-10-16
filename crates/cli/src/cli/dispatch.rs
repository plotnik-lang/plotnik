use std::path::PathBuf;

use clap::ArgMatches;

use super::ColorChoice;
use crate::commands::check::CheckArgs;
use crate::commands::dump::DumpArgs;
use crate::commands::generate::{GenerateArgs, GenerateTarget};
use crate::commands::language::LanguageOptions;

pub struct CheckOpts {
    pub query_path: PathBuf,
    pub language: LanguageOptions,
    pub strict: bool,
    pub color: ColorChoice,
}

impl CheckOpts {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            query_path: m
                .get_one::<PathBuf>("query_path")
                .expect("clap requires PATH")
                .clone(),
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
            language: p.language,
            strict: p.strict,
            color: p.color.should_colorize(),
        }
    }
}

pub struct DumpOpts {
    pub query_path: PathBuf,
    pub language: LanguageOptions,
    pub color: ColorChoice,
}

impl DumpOpts {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            query_path: m
                .get_one::<PathBuf>("query_path")
                .expect("clap requires PATH")
                .clone(),
            language: LanguageOptions::from_matches(m),
            color: ColorChoice::from_matches(m),
        }
    }
}

impl From<DumpOpts> for DumpArgs {
    fn from(p: DumpOpts) -> Self {
        Self {
            query_path: p.query_path,
            language: p.language,
            color: p.color.should_colorize(),
        }
    }
}

pub struct GenerateOpts {
    pub query_path: PathBuf,
    pub language: LanguageOptions,
    pub target: GenerateTarget,
    pub output: Option<PathBuf>,
    pub color: ColorChoice,
}

impl GenerateOpts {
    pub fn from_matches(m: &ArgMatches) -> Self {
        Self {
            query_path: m
                .get_one::<PathBuf>("query_path")
                .expect("clap requires PATH")
                .clone(),
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
            language: p.language,
            target: p.target,
            output: p.output,
            color: p.color.should_colorize(),
        }
    }
}
