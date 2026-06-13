//! Uniform exit codes across all commands:
//!
//! - `0` — yes/success (match found, query valid, tests pass)
//! - `1` — domain "no" (run: no match; check: invalid; test: failures)
//! - `2` — couldn't answer (usage, IO, internal error)
//!
//! Clap usage errors also exit with `2` (its default), keeping the contract whole.

use std::process::ExitCode;

pub type CliResult = Result<(), CliError>;

#[derive(Debug)]
pub enum CliError {
    /// The domain answer is "no". Any explanation has already been printed.
    No,
    /// Couldn't answer. The message is printed by `main` as `error: …`.
    Fatal(String),
    /// Couldn't answer; explanation (e.g. rendered diagnostics) already printed.
    FatalRendered,
}

impl CliError {
    pub fn fatal(msg: impl Into<String>) -> Self {
        Self::Fatal(msg.into())
    }

    pub fn report(self) -> ExitCode {
        match self {
            Self::No => ExitCode::from(1),
            Self::FatalRendered => ExitCode::from(2),
            Self::Fatal(msg) => {
                eprintln!("error: {msg}");
                ExitCode::from(2)
            }
        }
    }
}
