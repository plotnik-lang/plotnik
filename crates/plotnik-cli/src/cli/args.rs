//! Shared argument builders for CLI commands.
//!
//! Each function returns a `clap::Arg` that can be composed into commands.
//! This allows the same arg definition to be reused across commands with
//! different visibility settings (via `.hide(true)`).

use std::path::PathBuf;

use clap::{Arg, ArgAction, value_parser};

/// Query file or workspace directory (positional).
pub fn query_path_arg() -> Arg {
    Arg::new("query_path")
        .value_name("QUERY")
        .value_parser(value_parser!(PathBuf))
        .help("Query file or workspace directory")
}

/// Inline query text (-q/--query).
pub fn query_text_arg() -> Arg {
    Arg::new("query_text")
        .short('q')
        .long("query")
        .value_name("TEXT")
        .help("Inline query text")
}

/// Source file to parse/execute against (positional).
pub fn source_path_arg() -> Arg {
    Arg::new("source_path")
        .value_name("SOURCE")
        .value_parser(value_parser!(PathBuf))
        .help("Source file to parse")
}

/// Inline source text (-s/--source).
pub fn source_text_arg() -> Arg {
    Arg::new("source_text")
        .short('s')
        .long("source")
        .value_name("TEXT")
        .help("Inline source text")
}

/// Language flag (-l/--lang).
pub fn lang_arg() -> Arg {
    Arg::new("lang")
        .short('l')
        .long("lang")
        .value_name("LANG")
        .help("Language (inferred from extension if not specified)")
}

/// Color output control (--color).
pub fn color_arg() -> Arg {
    Arg::new("color")
        .long("color")
        .value_name("WHEN")
        .default_value("auto")
        .value_parser(["auto", "always", "never"])
        .help("Colorize output")
}

/// Include anonymous nodes (--raw).
pub fn raw_arg() -> Arg {
    Arg::new("raw")
        .long("raw")
        .action(ArgAction::SetTrue)
        .help("Include anonymous nodes (literals, punctuation)")
}

/// Show source positions (--spans).
pub fn spans_arg() -> Arg {
    Arg::new("spans")
        .long("spans")
        .action(ArgAction::SetTrue)
        .help("Show source positions")
}

/// Treat warnings as errors (--strict).
pub fn strict_arg() -> Arg {
    Arg::new("strict")
        .long("strict")
        .action(ArgAction::SetTrue)
        .help("Treat warnings as errors")
}

/// Output format (--format).
pub fn format_arg() -> Arg {
    Arg::new("format")
        .long("format")
        .value_name("FORMAT")
        .default_value("typescript")
        .help("Output format (typescript, ts)")
}

/// Use verbose node shape (--verbose-nodes).
/// Also used by exec command.
pub fn verbose_nodes_arg() -> Arg {
    Arg::new("verbose_nodes")
        .long("verbose-nodes")
        .action(ArgAction::SetTrue)
        .help("Include verbose node information (line/column positions)")
}

/// Don't emit Node/Point type definitions (--no-node-type).
pub fn no_node_type_arg() -> Arg {
    Arg::new("no_node_type")
        .long("no-node-type")
        .action(ArgAction::SetTrue)
        .help("Don't emit Node/Point type definitions")
}

/// Don't export types (--no-export).
pub fn no_export_arg() -> Arg {
    Arg::new("no_export")
        .long("no-export")
        .action(ArgAction::SetTrue)
        .help("Don't export types")
}

/// Type for void results (--void-type).
pub fn void_type_arg() -> Arg {
    Arg::new("void_type")
        .long("void-type")
        .value_name("TYPE")
        .help("Type for void results: undefined (default) or null")
}

/// Write output to file (-o/--output).
pub fn output_file_arg() -> Arg {
    Arg::new("output")
        .short('o')
        .long("output")
        .value_name("FILE")
        .value_parser(value_parser!(PathBuf))
        .help("Write output to file")
}

/// Output compact JSON (--compact).
pub fn compact_arg() -> Arg {
    Arg::new("compact")
        .long("compact")
        .action(ArgAction::SetTrue)
        .help("Output compact JSON (default: pretty when stdout is a TTY)")
}

/// Validate output against inferred types (--check).
pub fn check_arg() -> Arg {
    Arg::new("check")
        .long("check")
        .action(ArgAction::SetTrue)
        .help("Validate output against inferred types")
}

/// Entry point name (--entry).
/// Used by both exec and trace.
pub fn entry_arg() -> Arg {
    Arg::new("entry")
        .long("entry")
        .value_name("NAME")
        .help("Entry point name (definition to match from)")
}

/// Verbosity level (-v, -vv).
pub fn verbose_arg() -> Arg {
    Arg::new("verbose")
        .short('v')
        .action(ArgAction::Count)
        .help("Verbosity level (-v for verbose, -vv for very verbose)")
}

/// Skip materialization (--no-result).
pub fn no_result_arg() -> Arg {
    Arg::new("no_result")
        .long("no-result")
        .action(ArgAction::SetTrue)
        .help("Skip materialization, show effects only")
}

/// Execution fuel limit (--fuel).
pub fn fuel_arg() -> Arg {
    Arg::new("fuel")
        .long("fuel")
        .value_name("N")
        .default_value("1000000")
        .value_parser(value_parser!(u32))
        .help("Execution fuel limit")
}
