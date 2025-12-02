use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

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
    #[command(after_help = r#"OUTPUT DEPENDENCIES:

EXAMPLES:
  # Parse and typecheck query only
  plotnik debug --query-text '(identifier) @id' --query-cst --query-symbols

  # Dump tree-sitter AST of source file
  plotnik debug --source-file app.ts --source-ast



  # Full pipeline: match query against source
  plotnik debug --query-file rules.pql --source-file app.ts --result

  # Debug with trace
  plotnik debug --query-text '(function_declaration) @fn' \
          --source-text 'function foo() {}' --lang typescript --trace"#)]
    Debug {
        #[command(flatten)]
        query: QueryArgs,

        #[command(flatten)]
        source: SourceArgs,

        /// Language for source (required for --source-text, inferred from extension for --source-file)
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
    #[arg(long, value_name = "QUERY")]
    pub query_text: Option<String>,

    /// Query from file (use "-" for stdin)
    #[arg(long, value_name = "FILE")]
    pub query_file: Option<PathBuf>,
}

#[derive(Args)]
#[group(id = "source_input", multiple = false)]
pub struct SourceArgs {
    /// Source code as inline text
    #[arg(long, value_name = "SOURCE")]
    pub source_text: Option<String>,

    /// Source code from file (use "-" for stdin)
    #[arg(long, value_name = "FILE")]
    pub source_file: Option<PathBuf>,
}

#[derive(Args)]
pub struct OutputArgs {
    /// Show parsed query CST (concrete syntax tree)
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    pub query_cst: bool,

    /// Show parsed query AST (abstract syntax tree, semantic structure)
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    pub query_ast: bool,

    /// Show name resolution (definitions and references)
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    pub query_symbols: bool,

    /// Show inferred output types
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    pub query_types: bool,

    /// Show tree-sitter AST of source (semantic nodes only)
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    pub source_ast: bool,

    /// Show tree-sitter AST of source (all nodes including anonymous)
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    pub source_ast_full: bool,

    /// Show execution trace
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    pub trace: bool,

    /// Show match results
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    pub result: bool,
}
