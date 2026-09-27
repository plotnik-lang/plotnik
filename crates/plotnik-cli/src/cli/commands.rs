//! Command builders for the CLI.
//!
//! Each command is built using the shared arg builders from `args.rs`.
//! Runtime command arguments remain available even while their handlers are stubs.

use clap::Command;

use super::args::*;
use super::limits::{fuel_arg, limits_preset_arg, max_memory_arg};

fn with_hidden_exec_args(cmd: Command) -> Command {
    cmd.arg(entry_arg().hide(true))
        .arg(compact_arg().hide(true))
        .arg(include_points_arg().hide(true))
}

fn with_hidden_trace_args(cmd: Command) -> Command {
    cmd.arg(verbose_arg().hide(true))
        .arg(no_result_arg().hide(true))
}

// The tree command retains its hidden runtime-limit arguments.
fn with_hidden_runtime_limit_args(cmd: Command) -> Command {
    cmd.arg(fuel_arg().hide(true))
        .arg(max_memory_arg().hide(true))
        .arg(limits_preset_arg().hide(true))
}

fn with_hidden_json_arg(cmd: Command) -> Command {
    cmd.arg(json_arg().hide(true))
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
        .subcommand(inspect_command())
        .subcommand(lang_command())
        .subcommand(completions_command())
}

pub fn generate_command() -> Command {
    Command::new("gen")
        .visible_alias("generate")
        .about("Generate a compiled matcher module")
        .override_usage(
            "\
  plotnik gen <QUERY> --grammar <grammar.json> --target rust
  plotnik gen -q <TEXT> --grammar <grammar.json> --target rust",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik gen query.ptk --grammar path/to/grammar.json --target rust
  plotnik gen query.ptk --grammar path/to/grammar.json --target rust -o query.rs

The generated module imports `plotnik_rt`. Depend on `plotnik-rt` for
Tree-sitter or `plotnik-rt-arborium` for Arborium; both packages expose that
crate name. The module records the exact grammar name, SHA-256, and source used
during binding."#,
        )
        .arg(query_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(grammar_arg())
        .next_help_heading("Generation options")
        .arg(target_arg())
        .arg(debug_arg())
        .arg(output_file_arg())
        .next_help_heading("Global options")
        .arg(color_arg())
}

pub fn tree_command() -> Command {
    let cmd = Command::new("tree")
        .about("Query and source tree view (currently unimplemented)")
        .override_usage(
            "\
  plotnik tree <FILE>                 # auto-detect by extension
  plotnik tree <QUERY> <SOURCE>       # both trees
  plotnik tree -q <TEXT> [SOURCE]
  plotnik tree -s <TEXT> -l <LANG>",
        )
        .after_help("This command is currently unimplemented.")
        .arg(query_path_arg())
        .arg(source_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(source_text_arg())
        .arg(lang_arg())
        .next_help_heading("Output options")
        .arg(query_view_arg())
        .arg(include_anonymous_arg())
        .arg(json_arg().help("Output trees as JSON"))
        .next_help_heading("Global options")
        .arg(color_arg());

    with_hidden_runtime_limit_args(with_hidden_trace_args(with_hidden_exec_args(cmd)))
}

pub fn check_command() -> Command {
    Command::new("check")
        .about("Validate a query")
        .override_usage(
            "\
  plotnik check <QUERY> --grammar <grammar.json>
  plotnik check -q <TEXT> --grammar <grammar.json>",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik check query.ptk --grammar path/to/grammar.json
  plotnik check queries/ --grammar path/to/grammar.json
  plotnik check -q 'Q = ...' --grammar path/to/grammar.json --json"#,
        )
        .arg(query_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(grammar_arg())
        .next_help_heading("Check options")
        .arg(strict_arg())
        .arg(json_arg())
        .next_help_heading("Global options")
        .arg(color_arg())
}

pub fn dump_command() -> Command {
    Command::new("dump")
        .about("Show compiled bytecode for debugging")
        .override_usage(
            "\
  plotnik dump <QUERY> --grammar <grammar.json>
  plotnik dump -q <TEXT> --grammar <grammar.json>",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik dump query.ptk --grammar path/to/grammar.json
  plotnik dump -q 'Q = ...' --grammar path/to/grammar.json"#,
        )
        .arg(query_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(grammar_arg())
        .next_help_heading("Global options")
        .arg(color_arg())
}

pub fn infer_command() -> Command {
    Command::new("infer")
        .about("Generate type definitions from a query")
        .override_usage(
            "\
  plotnik infer <QUERY> --grammar <grammar.json>
  plotnik infer -q <TEXT> --grammar <grammar.json>",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik infer query.ptk --grammar path/to/grammar.json
  plotnik infer -q 'Q = ...' --grammar path/to/grammar.json
  plotnik infer query.ptk --grammar path/to/grammar.json -o types.d.ts"#,
        )
        .arg(query_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(grammar_arg())
        .next_help_heading("Output options")
        .arg(format_arg())
        .arg(include_points_arg())
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
        .about("Execute a query (currently unimplemented)")
        .override_usage(
            "\
  plotnik run <QUERY> <SOURCE>
  plotnik run -q <TEXT> <SOURCE>
  plotnik run -q <TEXT> -s <TEXT> -l <LANG>",
        )
        .after_help("This command is currently unimplemented.")
        .arg(query_path_arg())
        .arg(source_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(source_text_arg())
        .arg(lang_arg())
        .arg(entry_arg())
        .next_help_heading("Output options")
        .arg(compact_arg())
        .arg(include_points_arg().hide(true))
        .next_help_heading("Limit options")
        .arg(fuel_arg())
        .arg(max_memory_arg())
        .arg(limits_preset_arg())
        .next_help_heading("Global options")
        .arg(color_arg());

    with_hidden_json_arg(with_hidden_trace_args(cmd))
}

pub fn trace_command() -> Command {
    let cmd = Command::new("trace")
        .about("Trace query execution (currently unimplemented)")
        .override_usage(
            "\
  plotnik trace <QUERY> <SOURCE>
  plotnik trace -q <TEXT> <SOURCE>
  plotnik trace -q <TEXT> -s <TEXT> -l <LANG>",
        )
        .after_help("This command is currently unimplemented.")
        .arg(query_path_arg())
        .arg(source_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(source_text_arg())
        .arg(lang_arg())
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

    with_hidden_json_arg(
        cmd.arg(compact_arg().hide(true))
            .arg(include_points_arg().hide(true)),
    )
}

pub fn inspect_command() -> Command {
    let cmd = Command::new("inspect")
        .about("Inspect query execution (currently unimplemented)")
        .override_usage(
            "\
  plotnik inspect <QUERY> <SOURCE> [--json]
  plotnik inspect -q <TEXT> <SOURCE> [--json]
  plotnik inspect -q <TEXT> -s <TEXT> -l <LANG> [--json]",
        )
        .after_help("This command is currently unimplemented.")
        .arg(query_path_arg())
        .arg(source_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(source_text_arg())
        .arg(lang_arg())
        .arg(entry_arg())
        .next_help_heading("Inspect options")
        .arg(json_arg().help("Output the full inspect bundle as JSON"))
        .arg(verbose_arg().help("Include the VM execution trace in the JSON bundle"))
        .next_help_heading("Limit options")
        .arg(fuel_arg())
        .arg(max_memory_arg())
        .arg(limits_preset_arg())
        .next_help_heading("Global options")
        .arg(color_arg());

    cmd.arg(compact_arg().hide(true))
        .arg(include_points_arg().hide(true))
        .arg(no_result_arg().hide(true))
}

pub fn lang_command() -> Command {
    Command::new("lang")
        .about("Language information (currently unimplemented)")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .flatten_help(true)
        .subcommand(lang_list_command())
        .subcommand(lang_dump_command())
}

fn lang_list_command() -> Command {
    Command::new("list").about("List languages (currently unimplemented)")
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
        .about("Dump grammar tree shapes (currently unimplemented)")
        .arg(
            clap::Arg::new("lang")
                .help("Language name or alias")
                .required(true)
                .index(1),
        )
        .arg(
            clap::Arg::new("no-legend")
                .long("no-legend")
                .help("Omit the legend header")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("json")
                .long("json")
                .help("Emit the raw grammar.json instead of tree shapes")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("width")
                .long("width")
                .help("Fold groups inline up to this column width (0 = always break)")
                .value_parser(clap::value_parser!(usize)),
        )
}
