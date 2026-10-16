use std::path::PathBuf;

use clap::ArgMatches;
use plotnik_compiler::dump_tree_text;

use super::{inputs::load_source, language::LanguageOptions};
use crate::error::{CliError, CliResult, write_stdout};

pub fn run(m: &ArgMatches) -> CliResult {
    let path = m
        .get_one::<PathBuf>("input_path")
        .expect("clap requires FILE");
    let language = LanguageOptions::from_matches(m);
    if path.extension().is_none() && language.lang.is_none() {
        return Err(CliError::fatal("source language required: use --lang"));
    }

    let source = load_source(path)?;
    let mut parser = language.load(Some(path))?.parser()?;
    let tree = parser
        .parse(&source)
        .map_err(|error| CliError::fatal(error.to_string()))?;
    let anonymous = m.get_flag("include_anonymous");
    write_stdout(format_args!(
        "{}",
        dump_tree_text(&tree, &source, parser.grammar(), anonymous)
    ))
}
