use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
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
            ColorChoice::Auto => std::io::IsTerminal::is_terminal(&std::io::stderr()),
        }
    }
}

#[derive(Parser)]
#[command(name = "plotnik", bin_name = "plotnik")]
#[command(about = "Query language for tree-sitter AST with type inference")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Debug and inspect queries and source files
    #[command(after_help = r#"EXAMPLES:
  plotnik debug -q '(identifier) @id' --show-query
  plotnik debug -q '(identifier) @id' --only-symbols
  plotnik debug -s app.ts --show-source
  plotnik debug -s app.ts --show-source --raw
  plotnik debug -q '(function_declaration) @fn' -s app.ts -l typescript --show-query"#)]
    Debug {
        #[command(flatten)]
        query: QueryArgs,

        #[command(flatten)]
        source: SourceArgs,

        /// Language for source (required for inline text, inferred from extension otherwise)
        #[arg(long, short = 'l', value_name = "LANG")]
        lang: Option<String>,

        #[command(flatten)]
        output: OutputArgs,
    },

    /// Print documentation
    Docs {
        /// Topic to display (e.g., "reference", "examples")
        topic: Option<String>,
    },

    /// List supported languages
    Langs,
}

#[derive(Args)]
#[group(id = "query_input", multiple = false)]
pub struct QueryArgs {
    /// Query as inline text
    #[arg(short = 'q', long = "query", value_name = "QUERY")]
    pub query_text: Option<String>,

    /// Query from file (use "-" for stdin)
    #[arg(long = "query-file", value_name = "FILE")]
    pub query_file: Option<PathBuf>,
}

#[derive(Args)]
#[group(id = "source_input", multiple = false)]
pub struct SourceArgs {
    /// Source code as inline text
    #[arg(long = "source", value_name = "SOURCE")]
    pub source_text: Option<String>,

    /// Source code from file (use "-" for stdin)
    #[arg(short = 's', long = "source-file", value_name = "FILE")]
    pub source_file: Option<PathBuf>,
}

#[derive(Args)]
pub struct OutputArgs {
    /// Show query syntax tree
    #[arg(long = "show-query")]
    pub query: bool,

    /// Colorize output (auto-detected by default)
    #[arg(long, default_value = "auto", value_name = "WHEN")]
    pub color: ColorChoice,

    /// Show source syntax tree
    #[arg(long = "show-source")]
    pub source: bool,

    /// Show only symbol table
    #[arg(long = "only-symbols")]
    pub symbols: bool,

    /// Show query CST instead of AST (no effect on source)
    #[arg(long)]
    pub cst: bool,

    /// Include trivia tokens (whitespace, comments)
    #[arg(long)]
    pub raw: bool,

    /// Show source spans
    #[arg(long)]
    pub spans: bool,

    /// Show inferred cardinalities
    #[arg(long)]
    pub cardinalities: bool,
}
