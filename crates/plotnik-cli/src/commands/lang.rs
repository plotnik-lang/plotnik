use clap::ArgMatches;
use plotnik_lib::grammar::DumpOptions;

use super::language;
use crate::error::{CliError, CliResult, write_stdout};

pub fn run_dump(m: &ArgMatches) -> CliResult {
    let artifacts = language::load(m, None)?;
    if m.get_flag("json") {
        let json = std::str::from_utf8(&artifacts.json)
            .map_err(|error| CliError::fatal(error.to_string()))?;
        return write_stdout(format_args!("{json}"));
    }
    let options = DumpOptions {
        legend: !m.get_flag("no-legend"),
        width: m
            .get_one::<usize>("width")
            .copied()
            .unwrap_or(plotnik_lib::grammar::DEFAULT_WIDTH),
    };
    write_stdout(format_args!("{}", artifacts.grammar.tree().dump(&options)))
}
