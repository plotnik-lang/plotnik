//! Uniform exit codes across all commands:
//!
//! - `0` — success
//! - `1` — invalid query or emission failure
//! - `2` — couldn't answer (usage, IO, internal error)
//!
//! Clap usage errors also exit with `2` (its default), keeping the contract whole.

use std::fmt;
use std::io::{self, Write as _};
use std::process::ExitCode;

pub type CliResult = Result<(), CliError>;

pub fn write_stdout(args: fmt::Arguments<'_>) -> CliResult {
    io::stdout()
        .lock()
        .write_fmt(args)
        .map_err(|error| CliError::fatal(format!("failed to write stdout: {error}")))
}

pub fn writeln_stdout(args: fmt::Arguments<'_>) -> CliResult {
    let mut stdout = io::stdout().lock();
    stdout
        .write_fmt(args)
        .and_then(|()| stdout.write_all(b"\n"))
        .map_err(|error| CliError::fatal(format!("failed to write stdout: {error}")))
}

pub fn write_stderr(args: fmt::Arguments<'_>) -> CliResult {
    io::stderr()
        .lock()
        .write_fmt(args)
        .map_err(|error| CliError::fatal(format!("failed to write stderr: {error}")))
}

pub fn writeln_stderr(args: fmt::Arguments<'_>) -> CliResult {
    let mut stderr = io::stderr().lock();
    stderr
        .write_fmt(args)
        .and_then(|()| stderr.write_all(b"\n"))
        .map_err(|error| CliError::fatal(format!("failed to write stderr: {error}")))
}

#[derive(Debug)]
pub enum CliError {
    /// The domain answer is "no". Any explanation has already been printed.
    No,
    /// Couldn't answer. The message is printed by `main` as `error: …`.
    Fatal(String),
}

impl CliError {
    pub fn fatal(msg: impl Into<String>) -> Self {
        Self::Fatal(msg.into())
    }

    pub fn report(self) -> ExitCode {
        match self {
            Self::No => ExitCode::from(1),
            Self::Fatal(msg) => {
                let _ = writeln!(io::stderr().lock(), "error: {msg}");
                ExitCode::from(2)
            }
        }
    }
}
