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
    /// Explore a source file's tree-sitter AST
    #[command(
        override_usage = "\
  plotnik tree <SOURCE>
  plotnik tree -s <TEXT> -l <LANG>",
        after_help = r#"EXAMPLES:
  plotnik tree app.ts                 # source file
  plotnik tree app.ts --raw           # include anonymous nodes
  plotnik tree -s 'let x = 1' -l js   # inline source"#
    )]
    Tree {
        /// Source file to parse (use "-" for stdin)
        #[arg(value_name = "SOURCE")]
        source_path: Option<PathBuf>,

        /// Inline source text
        #[arg(short = 's', long = "source", value_name = "TEXT")]
        source_text: Option<String>,

        /// Language (inferred from extension if not specified)
        #[arg(short = 'l', long, value_name = "LANG")]
        lang: Option<String>,

        /// Include anonymous nodes (literals, punctuation)
        #[arg(long)]
        raw: bool,

        /// Show source positions
        #[arg(long)]
        spans: bool,
    },

    /// Validate a query
    #[command(
        override_usage = "\
  plotnik check <QUERY>
  plotnik check <QUERY> -l <LANG>
  plotnik check -q <TEXT> [-l <LANG>]",
        after_help = r#"EXAMPLES:
  plotnik check query.ptk             # validate syntax only
  plotnik check query.ptk -l ts       # also check against grammar
  plotnik check queries.ts/           # workspace directory
  plotnik check -q 'Q = ...' -l js    # inline query"#
    )]
    Check {
        /// Query file or workspace directory
        #[arg(value_name = "QUERY")]
        query_path: Option<PathBuf>,

        /// Inline query text
        #[arg(short = 'q', long = "query", value_name = "TEXT")]
        query_text: Option<String>,

        /// Language for grammar validation (inferred from workspace name if possible)
        #[arg(short = 'l', long, value_name = "LANG")]
        lang: Option<String>,

        /// Treat warnings as errors
        #[arg(long)]
        strict: bool,

        #[command(flatten)]
        output: OutputArgs,
    },

    /// Show compiled bytecode
    #[command(
        override_usage = "\
  plotnik dump <QUERY>
  plotnik dump <QUERY> -l <LANG>
  plotnik dump -q <TEXT> [-l <LANG>]",
        after_help = r#"EXAMPLES:
  plotnik dump query.ptk             # unlinked bytecode
  plotnik dump query.ptk -l ts       # linked (resolved node types)
  plotnik dump -q 'Q = ...'          # inline query"#
    )]
    Dump {
        /// Query file or workspace directory
        #[arg(value_name = "QUERY")]
        query_path: Option<PathBuf>,

        /// Inline query text
        #[arg(short = 'q', long = "query", value_name = "TEXT")]
        query_text: Option<String>,

        /// Language for linking (inferred from workspace name if possible)
        #[arg(short = 'l', long, value_name = "LANG")]
        lang: Option<String>,

        #[command(flatten)]
        output: OutputArgs,
    },

    /// Generate type definitions from a query
    #[command(
        override_usage = "\
  plotnik infer <QUERY> -l <LANG>
  plotnik infer -q <TEXT> -l <LANG>",
        after_help = r#"EXAMPLES:
  plotnik infer query.ptk -l js       # from file
  plotnik infer -q 'Q = ...' -l ts    # inline query
  plotnik infer query.ptk -l js -o types.d.ts  # write to file

NOTE: Use --verbose-nodes to match `exec --verbose-nodes` output shape."#
    )]
    Infer {
        /// Query file or workspace directory
        #[arg(value_name = "QUERY")]
        query_path: Option<PathBuf>,

        /// Inline query text
        #[arg(short = 'q', long = "query", value_name = "TEXT")]
        query_text: Option<String>,

        /// Target language (required, or inferred from workspace name)
        #[arg(short = 'l', long, value_name = "LANG")]
        lang: Option<String>,

        #[command(flatten)]
        infer_output: InferOutputArgs,

        #[command(flatten)]
        output: OutputArgs,
    },

    /// Execute a query against source code and output JSON
    #[command(
        override_usage = "\
  plotnik exec <QUERY> <SOURCE>
  plotnik exec -q <TEXT> <SOURCE>
  plotnik exec -q <TEXT> -s <TEXT> -l <LANG>",
        after_help = r#"EXAMPLES:
  plotnik exec query.ptk app.js           # two positional files
  plotnik exec -q 'Q = ...' app.js        # inline query + source file
  plotnik exec -q 'Q = ...' -s 'let x' -l js  # all inline"#
    )]
    Exec {
        /// Query file or workspace directory
        #[arg(value_name = "QUERY")]
        query_path: Option<PathBuf>,

        /// Source file to execute against
        #[arg(value_name = "SOURCE")]
        source_path: Option<PathBuf>,

        /// Inline query text
        #[arg(short = 'q', long = "query", value_name = "TEXT")]
        query_text: Option<String>,

        /// Inline source text
        #[arg(short = 's', long = "source", value_name = "TEXT")]
        source_text: Option<String>,

        /// Language (inferred from source extension if not specified)
        #[arg(short = 'l', long, value_name = "LANG")]
        lang: Option<String>,

        #[command(flatten)]
        exec_output: ExecOutputArgs,

        #[command(flatten)]
        output: OutputArgs,
    },

    /// Trace query execution for debugging
    #[command(
        override_usage = "\
  plotnik trace <QUERY> <SOURCE>
  plotnik trace -q <TEXT> <SOURCE>
  plotnik trace -q <TEXT> -s <TEXT> -l <LANG>",
        after_help = r#"EXAMPLES:
  plotnik trace query.ptk app.js          # two positional files
  plotnik trace -q 'Q = ...' app.js       # inline query + source file
  plotnik trace -q 'Q = ...' -s 'let x' -l js  # all inline"#
    )]
    Trace {
        /// Query file or workspace directory
        #[arg(value_name = "QUERY")]
        query_path: Option<PathBuf>,

        /// Source file to execute against
        #[arg(value_name = "SOURCE")]
        source_path: Option<PathBuf>,

        /// Inline query text
        #[arg(short = 'q', long = "query", value_name = "TEXT")]
        query_text: Option<String>,

        /// Inline source text
        #[arg(short = 's', long = "source", value_name = "TEXT")]
        source_text: Option<String>,

        /// Language (inferred from source extension if not specified)
        #[arg(short = 'l', long, value_name = "LANG")]
        lang: Option<String>,

        /// Entry point name (definition to match from)
        #[arg(long, value_name = "NAME")]
        entry: Option<String>,

        /// Verbosity level (-v for verbose, -vv for very verbose)
        #[arg(short = 'v', action = clap::ArgAction::Count)]
        verbose: u8,

        /// Skip materialization, show effects only
        #[arg(long)]
        no_result: bool,

        /// Execution fuel limit
        #[arg(long, default_value = "1000000", value_name = "N")]
        fuel: u32,

        #[command(flatten)]
        output: OutputArgs,
    },

    /// List supported languages
    Langs,
}

#[derive(Args)]
pub struct OutputArgs {
    /// Colorize output
    #[arg(long, default_value = "auto", value_name = "WHEN")]
    pub color: ColorChoice,
}

#[derive(Args)]
pub struct InferOutputArgs {
    /// Output format (typescript, ts)
    #[arg(long, default_value = "typescript", value_name = "FORMAT")]
    pub format: String,

    /// Use verbose node shape (matches exec --verbose-nodes)
    #[arg(long)]
    pub verbose_nodes: bool,

    /// Don't emit Node/Point type definitions
    #[arg(long)]
    pub no_node_type: bool,

    /// Don't export types
    #[arg(long)]
    pub no_export: bool,

    /// Type for void results: undefined (default) or null
    #[arg(long, value_name = "TYPE")]
    pub void_type: Option<String>,

    /// Write output to file
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,
}

#[derive(Args)]
pub struct ExecOutputArgs {
    /// Output compact JSON (default: pretty when stdout is a TTY)
    #[arg(long)]
    pub compact: bool,

    /// Include verbose node information (line/column positions)
    #[arg(long)]
    pub verbose_nodes: bool,

    /// Validate output against inferred types
    #[arg(long)]
    pub check: bool,

    /// Entry point name (definition to match from)
    #[arg(long, value_name = "NAME")]
    pub entry: Option<String>,
}
