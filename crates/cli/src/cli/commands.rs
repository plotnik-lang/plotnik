//! Command builders for the CLI.
//!
//! Each command is built using the shared arg builders from `args.rs`.

use std::path::PathBuf;

use clap::{Arg, ArgGroup, Command, value_parser};

use super::args::*;
use super::limits::{fuel_arg, limits_preset_arg, max_memory_arg};

fn with_hidden_exec_args(cmd: Command) -> Command {
    cmd.arg(entry_arg().hide(true))
        .arg(compact_arg().hide(true))
}

fn with_hidden_trace_args(cmd: Command) -> Command {
    cmd.arg(verbose_arg().hide(true))
        .arg(no_result_arg().hide(true))
}

// Hidden runtime-limit options for tree.
fn with_hidden_runtime_limit_args(cmd: Command) -> Command {
    cmd.arg(fuel_arg().hide(true))
        .arg(max_memory_arg().hide(true))
        .arg(limits_preset_arg().hide(true))
}

pub fn build_cli() -> Command {
    Command::new("plotnik")
        .about("Query language for tree-sitter syntax trees with type inference")
        .version(env!("CARGO_PKG_VERSION"))
        .propagate_version(true)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .after_help(
            r#"EXIT CODES:
  0  success
  1  invalid query or emission failure
  2  couldn't answer (usage, IO, or internal error)

Run 'plotnik <command> --help' for options."#,
        )
        .subcommand(run_command())
        .subcommand(check_command())
        .subcommand(tree_command())
        .subcommand(infer_command())
        .subcommand(generate_command())
        .subcommand(dump_command())
        .subcommand(trace_command())
        .subcommand(lang_command())
        .subcommand(completions_command())
}

pub fn generate_command() -> Command {
    Command::new("gen")
        .visible_alias("generate")
        .about("Generate a compiled matcher module")
        .override_usage(
            "\
  plotnik gen <PATH> [--lang <LANG>] --target rust
  plotnik gen -q <QUERY> [--lang <LANG>] --target rust",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik gen query.ptk --lang javascript --target rust
  plotnik gen query.ptk --lang javascript --target rust -o query.rs

The generated module imports `plotnik_runtime`. Depend on `plotnik-runtime` to use it.
The module records the exact grammar name, SHA-256, and source used during
binding."#,
        )
        .arg(query_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(lang_arg())
        .arg(cache_dir_arg())
        .next_help_heading("Generation options")
        .arg(target_arg())
        .arg(output_file_arg())
        .next_help_heading("Global options")
        .arg(color_arg())
}

pub fn tree_command() -> Command {
    let cmd = Command::new("tree")
        .about("Display the syntax tree")
        .override_usage(
            "\
  plotnik tree <FILE>                 # .ptk query or source file path
  plotnik tree - --lang <LANG>        # stdin, plotnik for query syntax
  plotnik tree -q <QUERY>
  plotnik tree -s <SOURCE> -l <LANG>",
        )
        .after_help(
            "Plotnik syntax is displayed locally.\n\
             Source syntax uses --lang or the source file extension.\n\
             Stdin and extensionless files require --lang; use plotnik for query syntax.",
        )
        .arg(
            Arg::new("input_path")
                .value_name("FILE")
                .value_parser(value_parser!(PathBuf))
                .help("File path (.ptk query or source), or '-' for stdin"),
        )
        .group(
            ArgGroup::new("input")
                .args(["input_path", "query_text", "source_text"])
                .required(true),
        )
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(source_text_arg())
        .arg(lang_arg())
        .arg(cache_dir_arg())
        .next_help_heading("Output options")
        .arg(query_view_arg())
        .arg(include_anonymous_arg())
        .next_help_heading("Global options")
        .arg(color_arg());

    with_hidden_runtime_limit_args(with_hidden_trace_args(with_hidden_exec_args(cmd)))
}

pub fn check_command() -> Command {
    Command::new("check")
        .about("Validate a query")
        .override_usage(
            "\
  plotnik check <PATH> [--lang <LANG>]
  plotnik check -q <QUERY> [--lang <LANG>]",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik check query.ptk --lang javascript
  plotnik check queries/ --lang javascript
  plotnik check -q 'Q = ...' --lang javascript

Individual queries can select a language with a first-line @language comment.
Directories require --lang."#,
        )
        .arg(query_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(lang_arg())
        .arg(cache_dir_arg())
        .next_help_heading("Check options")
        .arg(strict_arg())
        .next_help_heading("Global options")
        .arg(color_arg())
}

pub fn dump_command() -> Command {
    Command::new("dump")
        .about("Show compiled bytecode for debugging")
        .override_usage(
            "\
  plotnik dump <PATH> [--lang <LANG>]
  plotnik dump -q <QUERY> [--lang <LANG>]",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik dump query.ptk --lang javascript
  plotnik dump -q 'Q = ...' --lang javascript"#,
        )
        .arg(query_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(lang_arg())
        .arg(cache_dir_arg())
        .next_help_heading("Global options")
        .arg(color_arg())
}

pub fn infer_command() -> Command {
    Command::new("infer")
        .about("Generate type definitions from a query")
        .override_usage(
            "\
  plotnik infer <PATH> [--lang <LANG>]
  plotnik infer -q <QUERY> [--lang <LANG>]",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik infer query.ptk --lang javascript
  plotnik infer -q 'Q = ...' --lang javascript
  plotnik infer query.ptk --lang javascript -o types.d.ts"#,
        )
        .arg(query_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(lang_arg())
        .arg(cache_dir_arg())
        .next_help_heading("Output options")
        .arg(format_arg())
        .arg(no_node_type_arg())
        .arg(no_export_arg())
        .arg(match_only_type_arg())
        .arg(output_file_arg())
        .next_help_heading("Global options")
        .arg(color_arg())
}

pub fn run_command() -> Command {
    let cmd = Command::new("run")
        .alias("exec")
        .about("Execute a query")
        .override_usage(
            "\
  plotnik run <PATH> <FILE>
  plotnik run -q <QUERY> <FILE>
  plotnik run -q <QUERY> -s <SOURCE> -l <LANG>",
        )
        .arg(query_path_arg())
        .arg(source_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(source_text_arg())
        .arg(lang_arg())
        .arg(cache_dir_arg())
        .arg(entry_arg())
        .next_help_heading("Output options")
        .arg(compact_arg())
        .next_help_heading("Limit options")
        .arg(fuel_arg())
        .arg(max_memory_arg())
        .arg(limits_preset_arg())
        .next_help_heading("Global options")
        .arg(color_arg());

    with_hidden_trace_args(cmd)
}

pub fn trace_command() -> Command {
    let cmd = Command::new("trace")
        .about("Trace query execution")
        .override_usage(
            "\
  plotnik trace <PATH> <FILE>
  plotnik trace -q <QUERY> <FILE>
  plotnik trace -q <QUERY> -s <SOURCE> -l <LANG>",
        )
        .arg(query_path_arg())
        .arg(source_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(source_text_arg())
        .arg(lang_arg())
        .arg(cache_dir_arg())
        .arg(entry_arg())
        .next_help_heading("Trace options")
        .arg(verbose_arg())
        .arg(no_result_arg())
        .next_help_heading("Limit options")
        .arg(fuel_arg())
        .arg(max_memory_arg())
        .arg(limits_preset_arg())
        .next_help_heading("Global options")
        .arg(color_arg());

    cmd.arg(compact_arg().hide(true))
}

pub fn lang_command() -> Command {
    Command::new("lang")
        .about("Grammar information from plotnik.dev/lang")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .flatten_help(true)
        .subcommand(lang_dump_command())
}

pub fn completions_command() -> Command {
    Command::new("completions")
        .about("Generate shell completions")
        .after_help(
            r#"EXAMPLES:
  plotnik completions zsh > ~/.zfunc/_plotnik
  plotnik completions bash > /etc/bash_completion.d/plotnik"#,
        )
        .arg(
            clap::Arg::new("shell")
                .help("Shell to generate completions for")
                .required(true)
                .value_parser(clap::value_parser!(clap_complete::Shell)),
        )
}

fn lang_dump_command() -> Command {
    Command::new("dump")
        .about("Dump grammar tree shapes")
        .arg(cache_dir_arg())
        .arg(
            clap::Arg::new("lang")
                .value_name("LANG[@REVISION]")
                .help(LANGUAGE_HELP)
                .index(1),
        )
        .arg(
            clap::Arg::new("no-legend")
                .long("no-legend")
                .help("Omit the legend header")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("width")
                .long("width")
                .help("Fold groups inline up to this column width (0 = always break)")
                .value_parser(clap::value_parser!(usize)),
        )
}
