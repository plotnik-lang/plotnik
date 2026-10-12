use clap::ArgMatches;
use plotnik_compiler::grammar::DumpOptions;

use super::language;
use crate::error::{CliResult, write_stdout};

pub fn run_dump(m: &ArgMatches) -> CliResult {
    let artifacts = language::load(m, None)?;
    let options = DumpOptions {
        legend: !m.get_flag("no-legend"),
        width: m
            .get_one::<usize>("width")
            .copied()
            .unwrap_or(plotnik_compiler::grammar::DEFAULT_WIDTH),
    };
    write_stdout(format_args!("{}", artifacts.grammar.tree().dump(&options)))
}
