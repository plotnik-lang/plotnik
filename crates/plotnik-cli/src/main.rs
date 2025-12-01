use clap::{Args, Parser, Subcommand};
use plotnik_lib::Query;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "plotnik", bin_name = "plotnik")]
#[command(about = "Query language for tree-sitter AST with type inference")]
#[command(after_help = r#"OUTPUT DEPENDENCIES:
┌─────────────────┬─────────────┬──────────────┐
│ Output          │ Needs Query │ Needs Source │
├─────────────────┼─────────────┼──────────────┤
│ --query-cst     │      ✓      │              │
│ --query-ast     │      ✓      │              │
│ --query-refs    │      ✓      │              │
│ --query-types   │      ✓      │              │
│ --source-ast    │             │      ✓       │
│ --trace         │      ✓      │      ✓       │
│ --result        │      ✓      │      ✓       │
└─────────────────┴─────────────┴──────────────┘

EXAMPLES:
  # Parse and typecheck query only
  plotnik --query-text '(identifier) @id' --query-cst --query-types

  # Dump tree-sitter AST of source file
  plotnik --source-file app.ts --source-ast

  # Full pipeline: match query against source
  plotnik --query-file rules.pql --source-file app.ts --result

  # Debug with trace
  plotnik --query-text '(function_declaration) @fn' \
          --source-text 'function foo() {}' --lang typescript --trace

  # Show documentation
  plotnik docs reference"#)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    query: QueryArgs,

    #[command(flatten)]
    source: SourceArgs,

    /// Language for source (required for --source-text, inferred from extension for --source-file)
    #[arg(long, short = 'l', value_name = "LANG")]
    lang: Option<String>,

    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Subcommand)]
enum Command {
    /// Print documentation
    Docs {
        /// Topic to display (e.g., "reference", "examples")
        topic: Option<String>,
    },
}

#[derive(Args)]
#[group(id = "query_input", multiple = false)]
struct QueryArgs {
    /// Query as inline text
    #[arg(long, value_name = "QUERY")]
    query_text: Option<String>,

    /// Query from file (use "-" for stdin)
    #[arg(long, value_name = "FILE")]
    query_file: Option<PathBuf>,
}

#[derive(Args)]
#[group(id = "source_input", multiple = false)]
struct SourceArgs {
    /// Source code as inline text
    #[arg(long, value_name = "SOURCE")]
    source_text: Option<String>,

    /// Source code from file (use "-" for stdin)
    #[arg(long, value_name = "FILE")]
    source_file: Option<PathBuf>,
}

#[derive(Args)]
struct OutputArgs {
    /// Show parsed query CST (concrete syntax tree)
    #[arg(long)]
    query_cst: bool,

    /// Show parsed query AST (abstract syntax tree, semantic structure)
    #[arg(long)]
    query_ast: bool,

    /// Show name resolution (definitions and references)
    #[arg(long)]
    query_refs: bool,

    /// Show inferred output types
    #[arg(long)]
    query_types: bool,

    /// Show tree-sitter AST of source
    #[arg(long)]
    source_ast: bool,

    /// Show execution trace
    #[arg(long)]
    trace: bool,

    /// Show match results
    #[arg(long)]
    result: bool,
}

fn main() {
    let cli = Cli::parse();

    if let Some(Command::Docs { topic }) = cli.command {
        print_help(topic.as_deref());
        return;
    }

    let has_query = cli.query.query_text.is_some() || cli.query.query_file.is_some();
    let has_source = cli.source.source_text.is_some() || cli.source.source_file.is_some();

    // Validate output dependencies
    if (cli.output.query_cst
        || cli.output.query_ast
        || cli.output.query_refs
        || cli.output.query_types)
        && !has_query
    {
        eprintln!(
            "error: --query-cst, --query-ast, --query-refs, and --query-types require --query-text or --query-file"
        );
        std::process::exit(1);
    }

    if cli.output.source_ast && !has_source {
        eprintln!("error: --source-ast requires --source-text or --source-file");
        std::process::exit(1);
    }

    if cli.output.trace && !(has_query && has_source) {
        eprintln!("error: --trace requires both query and source inputs");
        std::process::exit(1);
    }

    if cli.output.result && !(has_query && has_source) {
        eprintln!("error: --result requires both query and source inputs");
        std::process::exit(1);
    }

    // If both inputs provided and no outputs selected, default to --result
    let show_result = cli.output.result
        || (has_query
            && has_source
            && !cli.output.query_cst
            && !cli.output.query_ast
            && !cli.output.query_refs
            && !cli.output.query_types
            && !cli.output.source_ast
            && !cli.output.trace);

    // Validate --lang requirement
    if cli.source.source_text.is_some() && cli.lang.is_none() {
        eprintln!("error: --lang is required when using --source-text");
        std::process::exit(1);
    }

    // Load query if needed
    let query_source = if has_query {
        Some(load_query(&cli.query))
    } else {
        None
    };

    let query = query_source.as_ref().map(|src| Query::new(src));

    if cli.output.query_cst {
        println!("=== QUERY CST ===");
        if let Some(ref q) = query {
            print!("{}", q.format_cst());
        }
    }

    if cli.output.query_ast {
        println!("=== QUERY AST ===");
        if let Some(ref q) = query {
            print!("{}", q.format_ast());
        }
    }

    if cli.output.query_refs {
        println!("=== QUERY REFS ===");
        if let Some(ref q) = query {
            print!("{}", q.format_refs());
        }
    }

    if cli.output.query_types {
        println!("=== QUERY TYPES ===");
        println!("(not implemented)");
        println!();
    }

    if cli.output.source_ast {
        println!("=== SOURCE AST ===");
        println!("(not implemented)");
        println!();
    }

    if cli.output.trace {
        println!("=== TRACE ===");
        println!("(not implemented)");
        println!();
    }

    if show_result {
        println!("=== RESULT ===");
        println!("(not implemented)");
        println!();
    }

    // Print query errors at the end, grouped by stage
    if let Some(ref q) = query {
        if !q.is_valid() {
            eprint!("{}", q.render_errors_grouped());
        }
    }
}

fn load_query(args: &QueryArgs) -> String {
    if let Some(ref text) = args.query_text {
        return text.clone();
    }
    if let Some(ref path) = args.query_file {
        if path.as_os_str() == "-" {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .expect("failed to read stdin");
            return buf;
        }
        return fs::read_to_string(path).expect("failed to read query file");
    }
    unreachable!()
}

fn print_help(topic: Option<&str>) {
    match topic {
        None => {
            println!("Available topics:");
            println!("  reference  - Query language reference");
            println!("  examples   - Example queries");
            println!();
            println!("Usage: plotnik docs <topic>");
        }
        Some("reference") => {
            // TODO: include_str! the actual REFERENCE.md
            println!("{}", include_str!("../../../docs/REFERENCE.md"));
        }
        Some("examples") => {
            println!("(examples not yet written)");
        }
        Some(other) => {
            eprintln!("Unknown help topic: {}", other);
            eprintln!("Run 'plotnik help' to see available topics");
            std::process::exit(1);
        }
    }
}
