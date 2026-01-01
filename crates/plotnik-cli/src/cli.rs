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
    #[command(after_help = r#"EXAMPLES:
  plotnik tree app.ts
  plotnik tree app.ts --raw
  plotnik tree app.ts --spans"#)]
    Tree {
        /// Source file to parse (use "-" for stdin)
        #[arg(value_name = "SOURCE")]
        source: PathBuf,

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
    #[command(after_help = r#"EXAMPLES:
  plotnik check query.ptk
  plotnik check query.ptk -l typescript
  plotnik check queries.ts/
  plotnik check -q '(identifier) @id' -l javascript"#)]
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
    #[command(after_help = r#"EXAMPLES:
  plotnik dump query.ptk
  plotnik dump query.ptk -l typescript
  plotnik dump -q '(identifier) @id'"#)]
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
    #[command(after_help = r#"EXAMPLES:
  plotnik infer query.ptk -l javascript
  plotnik infer queries.ts/ -o types.d.ts
  plotnik infer -q '(function_declaration) @fn' -l typescript
  plotnik infer query.ptk -l js --verbose-nodes

NOTE: Use --verbose-nodes to match `exec --verbose-nodes` output shape."#)]
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
    #[command(after_help = r#"EXAMPLES:
  plotnik exec query.ptk app.js
  plotnik exec -q '(identifier) @id' -s app.js
  plotnik exec query.ptk app.ts --pretty
  plotnik exec query.ptk app.ts --verbose-nodes"#)]
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

        /// Source code as inline text
        #[arg(long = "source", value_name = "TEXT")]
        source_text: Option<String>,

        /// Source file (alternative to positional)
        #[arg(short = 's', long = "source-file", value_name = "FILE")]
        source_file: Option<PathBuf>,

        /// Language (inferred from source extension if not specified)
        #[arg(short = 'l', long, value_name = "LANG")]
        lang: Option<String>,

        #[command(flatten)]
        exec_output: ExecOutputArgs,

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
    /// Pretty-print JSON output
    #[arg(long)]
    pub pretty: bool,

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
