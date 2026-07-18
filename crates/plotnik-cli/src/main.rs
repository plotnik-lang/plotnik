mod cli;
mod commands;
mod error;
mod language_registry;

use std::io::{self, Write as _};
use std::process::ExitCode;

use clap::ArgMatches;

use cli::{
    CheckOpts, DumpOpts, GenerateOpts, InferOpts, InspectOpts, LangDumpOpts, RunOpts, TraceOpts,
    TreeOpts, build_cli, route_default_subcommand,
};
use error::{CliError, CliResult};

fn main() -> ExitCode {
    // Die silently on closed pipes (`plotnik run … | head`) like standard Unix
    // tools, instead of panicking with exit 101 when println! hits EPIPE.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let args = route_default_subcommand(std::env::args_os().collect());
    let matches = build_cli().get_matches_from(args);

    match dispatch(&matches) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => e.report(),
    }
}

fn dispatch(matches: &ArgMatches) -> CliResult {
    match matches.subcommand() {
        Some(("tree", m)) => {
            let params = TreeOpts::from_matches(m);
            commands::tree::run(params.into())
        }
        Some(("check", m)) => {
            let params = CheckOpts::from_matches(m);
            commands::check::run(params.into())
        }
        Some(("dump", m)) => {
            let params = DumpOpts::from_matches(m);
            commands::dump::run(params.into())
        }
        Some(("infer", m)) => {
            let params = InferOpts::from_matches(m);
            commands::infer::run(params.into())
        }
        Some(("gen", m)) => {
            let params = GenerateOpts::from_matches(m);
            commands::generate::run(params.into())
        }
        Some(("run", m)) => {
            let params = RunOpts::from_matches(m);
            commands::run::run(params.into())
        }
        Some(("trace", m)) => {
            let params = TraceOpts::from_matches(m);
            commands::trace::run(params.into())
        }
        Some(("inspect", m)) => {
            let params = InspectOpts::from_matches(m);
            commands::inspect::run(params.into())
        }
        Some(("lang", m)) => match m.subcommand() {
            Some(("list", _)) => commands::lang::run_list(),
            Some(("dump", sub_m)) => {
                let params = LangDumpOpts::from_matches(sub_m);
                commands::lang::run_dump(&params.lang, params.legend, params.json, params.width)
            }
            _ => unreachable!("clap should have caught this"),
        },
        Some(("completions", m)) => {
            let shell = *m
                .get_one::<clap_complete::Shell>("shell")
                .expect("shell is required");
            let mut output = Vec::new();
            clap_complete::generate(shell, &mut build_cli(), "plotnik", &mut output);
            io::stdout().lock().write_all(&output).map_err(|error| {
                CliError::fatal(format!("failed to write completions: {error}"))
            })?;
            Ok(())
        }
        _ => unreachable!("clap should have caught this"),
    }
}
