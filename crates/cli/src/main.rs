mod cli;
mod commands;
mod error;

use std::io::{self, Write as _};
use std::process::ExitCode;

use clap::ArgMatches;

use cli::{CheckOpts, DumpOpts, GenerateOpts, InferOpts, build_cli};
use error::{CliError, CliResult};

fn main() -> ExitCode {
    // Rust ignores SIGPIPE by default. Restore silent Unix closed-pipe termination.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let matches = build_cli().get_matches();

    match dispatch(&matches) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => e.report(),
    }
}

fn dispatch(matches: &ArgMatches) -> CliResult {
    match matches.subcommand() {
        Some(("tree", m)) => commands::tree::run(m),
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
        Some((command @ ("exec" | "find" | "find-all"), m)) => commands::exec::run(m, command),
        Some(("lang", m)) => match m.subcommand() {
            Some(("dump", m)) => commands::lang::run_dump(m),
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
