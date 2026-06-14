mod args;
mod commands;
mod dispatch;
pub mod shebang;

#[cfg(test)]
mod dispatch_tests;
#[cfg(test)]
mod shebang_tests;

pub use commands::build_cli;
pub use dispatch::{
    AstParams, CheckParams, DumpParams, InferParams, LangDumpParams, LangListParams, RunParams,
    TraceParams,
};

/// Bare default subcommand: `plotnik query.ptk …` routes to `run`.
/// This is what makes flag-free shebang execution (`#!/usr/bin/env plotnik`) work.
pub fn route_default_subcommand(mut args: Vec<std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    if let Some(first) = args.get(1)
        && !first.as_encoded_bytes().starts_with(b"-")
        && std::path::Path::new(first)
            .extension()
            .is_some_and(|ext| ext == "ptk")
    {
        args.insert(1, "run".into());
    }
    args
}

/// Color output mode for CLI commands.
#[derive(Clone, Copy, Debug, Default)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorChoice {
    /// Parse the `--color` flag into a `ColorChoice`.
    pub fn from_matches(m: &clap::ArgMatches) -> Self {
        match m.get_one::<String>("color").map(|s| s.as_str()) {
            Some("always") => ColorChoice::Always,
            Some("never") => ColorChoice::Never,
            _ => ColorChoice::Auto,
        }
    }

    pub fn should_colorize(self) -> bool {
        match self {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                // https://no-color.org: any non-empty value disables color
                if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
                    return false;
                }
                // https://bixense.com/clicolors: force color even when piped
                if std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| !v.is_empty() && v != "0") {
                    return true;
                }
                if std::env::var_os("TERM").is_some_and(|t| t == "dumb") {
                    return false;
                }
                // Check both streams: if either is piped, disable colors.
                // This handles `run query.ptk app.js | jq` where stdout is piped
                // but stderr (diagnostics) is still a TTY.
                std::io::IsTerminal::is_terminal(&std::io::stdout())
                    && std::io::IsTerminal::is_terminal(&std::io::stderr())
            }
        }
    }
}
