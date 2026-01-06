mod args;
mod commands;
mod dispatch;

#[cfg(test)]
mod dispatch_tests;

pub use commands::build_cli;
pub use dispatch::{
    AstParams, CheckParams, DumpParams, ExecParams, InferParams, LangsParams, TraceParams,
};

/// Color output mode for CLI commands.
#[derive(Clone, Copy, Debug, Default)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorChoice {
    pub fn should_colorize(self) -> bool {
        match self {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            // Check both streams: if either is piped, disable colors.
            // This handles `exec query.ptk app.js | jq` where stdout is piped
            // but stderr (diagnostics) is still a TTY.
            ColorChoice::Auto => {
                std::io::IsTerminal::is_terminal(&std::io::stdout())
                    && std::io::IsTerminal::is_terminal(&std::io::stderr())
            }
        }
    }
}
