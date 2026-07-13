//! Shared argument builders for CLI commands.
//!
//! Each function returns a `clap::Arg` that can be composed into commands.
//! This allows the same arg definition to be reused across commands with
//! different visibility settings (via `.hide(true)`).

use std::path::PathBuf;

use clap::{Arg, ArgAction, value_parser};

use crate::commands::generate::GenerateTarget;

pub fn query_path_arg() -> Arg {
    Arg::new("query_path")
        .value_name("QUERY")
        .value_parser(value_parser!(PathBuf))
        .help("Query file or workspace directory")
}

pub fn query_text_arg() -> Arg {
    Arg::new("query_text")
        .short('q')
        .long("query")
        .value_name("TEXT")
        .help("Inline query text")
}

pub fn source_path_arg() -> Arg {
    Arg::new("source_path")
        .value_name("SOURCE")
        .value_parser(value_parser!(PathBuf))
        .help("Source file to parse")
}

pub fn source_text_arg() -> Arg {
    Arg::new("source_text")
        .short('s')
        .long("source")
        .value_name("TEXT")
        .help("Inline source text")
}

pub fn lang_arg() -> Arg {
    Arg::new("lang")
        .short('l')
        .long("lang")
        .value_name("LANG")
        .help("Language (inferred from extension if not specified)")
}

pub fn color_arg() -> Arg {
    Arg::new("color")
        .long("color")
        .value_name("WHEN")
        .default_value("auto")
        .value_parser(["auto", "always", "never"])
        .help("Colorize output")
}

pub fn json_arg() -> Arg {
    Arg::new("json")
        .long("json")
        .action(ArgAction::SetTrue)
        .help("Output diagnostics as JSON")
}

pub fn raw_arg() -> Arg {
    Arg::new("raw")
        .long("raw")
        .action(ArgAction::SetTrue)
        .help("Include anonymous nodes (literals, punctuation)")
}

pub fn query_view_arg() -> Arg {
    Arg::new("query_view")
        .long("query-view")
        .value_name("VIEW")
        .default_value("ast")
        .value_parser(["ast", "cst"])
        .help("Choose the query tree view")
}

pub fn include_anonymous_arg() -> Arg {
    Arg::new("include_anonymous")
        .long("include-anonymous")
        .action(ArgAction::SetTrue)
        .help("Include anonymous source-tree nodes such as literals and punctuation")
}

pub fn strict_arg() -> Arg {
    Arg::new("strict")
        .long("strict")
        .action(ArgAction::SetTrue)
        .help("Treat warnings as errors")
}

pub fn format_arg() -> Arg {
    Arg::new("format")
        .long("format")
        .value_name("FORMAT")
        .default_value("typescript")
        .help("Output format (typescript, ts)")
}

pub fn include_points_arg() -> Arg {
    Arg::new("include_points")
        .long("include-points")
        .action(ArgAction::SetTrue)
        .help("Include zero-based row/byte-column points in nodes")
}

pub fn no_node_type_arg() -> Arg {
    Arg::new("no_node_type")
        .long("no-node-type")
        .action(ArgAction::SetTrue)
        .help("Don't emit the Node type definition")
}

pub fn no_export_arg() -> Arg {
    Arg::new("no_export")
        .long("no-export")
        .action(ArgAction::SetTrue)
        .help("Don't export types")
}

pub fn match_only_type_arg() -> Arg {
    Arg::new("match_only_type")
        .long("match-only-type")
        .value_name("TYPE")
        .value_parser(["undefined", "null"])
        .help("Type for match-only results: undefined (default) or null")
}

pub fn output_file_arg() -> Arg {
    Arg::new("output")
        .short('o')
        .long("output")
        .value_name("FILE")
        .value_parser(value_parser!(PathBuf))
        .help("Write output to file")
}

pub fn target_arg() -> Arg {
    Arg::new("target")
        .long("target")
        .value_name("TARGET")
        .value_parser(value_parser!(GenerateTarget))
        .required(true)
        .help("Generated-code target (rust)")
}

pub fn grammar_arg() -> Arg {
    Arg::new("grammar")
        .long("grammar")
        .value_name("GRAMMAR_JSON")
        .value_parser(value_parser!(PathBuf))
        .conflicts_with("lang")
        .help("Bind query names using this exact grammar.json instead of the registry")
}

pub fn compact_arg() -> Arg {
    Arg::new("compact")
        .long("compact")
        .action(ArgAction::SetTrue)
        .help("Output compact JSON (default: pretty when stdout is a TTY)")
}

pub fn entry_arg() -> Arg {
    Arg::new("entry")
        .long("entry")
        .value_name("NAME")
        .help("Entry point name (definition to match from)")
}

pub fn verbose_arg() -> Arg {
    Arg::new("verbose")
        .short('v')
        .action(ArgAction::Count)
        .help("Verbosity level (-v for verbose, -vv for very verbose)")
}

pub fn no_result_arg() -> Arg {
    Arg::new("no_result")
        .long("no-result")
        .action(ArgAction::SetTrue)
        .help("Skip materialization, show the execution trace only")
}
