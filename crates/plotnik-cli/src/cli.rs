use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum OutputLang {
    #[default]
    Rust,
    Typescript,
    Ts,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum IndirectionChoice {
    #[default]
    Box,
    Rc,
    Arc,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum OptionalChoice {
    #[default]
    Null,
    Undefined,
    #[value(name = "questionmark")]
    QuestionMark,
}

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

    /// Infer and emit types from a query
    #[command(after_help = r#"EXAMPLES:
  plotnik infer -q '(identifier) @id' -l rust
  plotnik infer -q '(function_declaration name: (identifier) @name) @fn' -l ts --export
  plotnik infer --query-file query.plot -l rust --derive debug,clone,partialeq"#)]
    Infer {
        #[command(flatten)]
        query: QueryArgs,

        /// Output language
        #[arg(short = 'l', long, value_name = "LANG")]
        lang: OutputLang,

        #[command(flatten)]
        common: InferCommonArgs,

        #[command(flatten)]
        rust: RustArgs,

        #[command(flatten)]
        typescript: TypeScriptArgs,
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

    /// Show inferred cardinalities
    #[arg(long)]
    pub cardinalities: bool,
}

#[derive(Args)]
pub struct InferCommonArgs {
    /// Name for the entry point type (default: QueryResult)
    #[arg(long, value_name = "NAME")]
    pub entry_name: Option<String>,

    /// Colorize diagnostics output
    #[arg(long, default_value = "auto", value_name = "WHEN")]
    pub color: ColorChoice,
}

#[derive(Args)]
pub struct RustArgs {
    /// Indirection type for cyclic references
    #[arg(long, value_name = "TYPE")]
    pub indirection: Option<IndirectionChoice>,

    /// Derive macros (comma-separated: debug, clone, partialeq)
    #[arg(long, value_name = "TRAITS", value_delimiter = ',')]
    pub derive: Option<Vec<String>>,

    /// Emit no derive macros
    #[arg(long)]
    pub no_derive: bool,
}

#[derive(Args)]
pub struct TypeScriptArgs {
    /// How to represent optional values
    #[arg(long, value_name = "STYLE")]
    pub optional: Option<OptionalChoice>,

    /// Add export keyword to types
    #[arg(long)]
    pub export: bool,

    /// Make fields readonly
    #[arg(long)]
    pub readonly: bool,

    /// Use type aliases instead of interfaces
    #[arg(long)]
    pub type_alias: bool,

    /// Name for the Node type (default: SyntaxNode)
    #[arg(long, value_name = "NAME")]
    pub node_type: Option<String>,

    /// Emit nested synthetic types instead of inlining
    #[arg(long)]
    pub nested: bool,
}
