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
  plotnik debug -q '(identifier) @id'
  plotnik debug -q '(identifier) @id' --only-symbols
  plotnik debug -s app.ts
  plotnik debug -s app.ts --raw
  plotnik debug -q '(function_declaration) @fn' -s app.ts -l typescript"#)]
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

    /// List supported languages
    Langs,

    /// Execute a query against source code and output JSON
    #[command(after_help = r#"EXAMPLES:
  plotnik exec -q '(identifier) @id' -s app.js
  plotnik exec -q '(identifier) @id' -s app.js --pretty
  plotnik exec -q '(function_declaration) @fn' -s app.ts -l typescript --verbose-nodes
  plotnik exec -q '(identifier) @id' -s app.js --check
  plotnik exec --query-file query.ptk -s app.js --entry FunctionDef"#)]
    Exec {
        #[command(flatten)]
        query: QueryArgs,

        #[command(flatten)]
        source: SourceArgs,

        /// Language for source (required for inline text, inferred from extension otherwise)
        #[arg(long, short = 'l', value_name = "LANG")]
        lang: Option<String>,

        #[command(flatten)]
        output: ExecOutputArgs,
    },

    /// Generate type definitions from a query
    #[command(after_help = r#"EXAMPLES:
  plotnik types -q '(identifier) @id' -l javascript
  plotnik types --query-file query.ptk -l typescript
  plotnik types -q '(function_declaration) @fn' -l js --format ts
  plotnik types -q '(identifier) @id' -l js --verbose-nodes
  plotnik types -q '(identifier) @id' -l js -o types.d.ts

NOTE: Use --verbose-nodes to match `exec --verbose-nodes` output shape."#)]
    Types {
        #[command(flatten)]
        query: QueryArgs,

        /// Target language (required)
        #[arg(long, short = 'l', value_name = "LANG")]
        lang: Option<String>,

        #[command(flatten)]
        output: TypesOutputArgs,
    },
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

#[derive(Args)]
pub struct TypesOutputArgs {
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

    /// Write output to file
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,
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
    /// Colorize output (auto-detected by default)
    #[arg(long, default_value = "auto", value_name = "WHEN")]
    pub color: ColorChoice,

    /// Show only symbol table (instead of query AST)
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

    /// Show inferred arities
    #[arg(long)]
    pub arities: bool,

    /// Show compiled graph
    #[arg(long)]
    pub graph: bool,

    /// Show unoptimized graph (before epsilon elimination)
    #[arg(long)]
    pub graph_raw: bool,

    /// Show inferred types
    #[arg(long)]
    pub types: bool,
}
