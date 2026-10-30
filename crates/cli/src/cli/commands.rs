use std::path::PathBuf;

use clap::{Arg, Command, value_parser};

use super::args::*;

fn with_hidden_exec_args(cmd: Command) -> Command {
    cmd.arg(entry_arg().hide(true))
        .arg(compact_arg().hide(true))
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
        .subcommand(execution_command(
            "exec",
            "Apply a query at the source-tree root",
        ))
        .subcommand(execution_command(
            "find",
            "Find the first match in the source tree",
        ))
        .subcommand(execution_command(
            "find-all",
            "Collect one result per matching node",
        ))
        .subcommand(check_command())
        .subcommand(tree_command())
        .subcommand(generate_command())
        .subcommand(dump_command())
        .subcommand(lang_command())
        .subcommand(completions_command())
}

pub fn generate_command() -> Command {
    Command::new("gen")
        .about("Generate Rust matchers from a query folder")
        .after_help(
            r#"EXAMPLES:
  plotnik gen
  plotnik gen queries/ --lang javascript

Each .ptk file produces a .rs file beside it. Subfolders are not searched.
A first-line language tag selects the grammar. Use --lang for untagged files.
Add plotnik-runtime to your Rust dependencies to use the matchers."#,
        )
        .arg(
            Arg::new("query_path")
                .value_name("FOLDER")
                .value_parser(value_parser!(PathBuf))
                .default_value(".")
                .help("Query folder"),
        )
        .next_help_heading("Input options")
        .arg(lang_arg().help("Grammar for untagged files"))
        .arg(cache_dir_arg())
        .next_help_heading("Global options")
        .arg(color_arg())
}

pub fn tree_command() -> Command {
    let cmd = Command::new("tree")
        .about("Display a source-language syntax tree")
        .override_usage(
            "\
  plotnik tree <FILE> [--lang <LANG>] [--include-anonymous]
  plotnik tree - --lang <LANG> [--include-anonymous]",
        )
        .after_help(
            "The source file extension selects the grammar unless --lang is set.\n\
             Stdin and extensionless files require --lang.",
        )
        .arg(
            Arg::new("input_path")
                .value_name("FILE")
                .value_parser(value_parser!(PathBuf))
                .required(true)
                .help("Source file path, or '-' for stdin"),
        )
        .next_help_heading("Input options")
        .arg(lang_arg())
        .arg(cache_dir_arg())
        .next_help_heading("Output options")
        .arg(include_anonymous_arg())
        .next_help_heading("Global options")
        .arg(color_arg());

    with_hidden_exec_args(cmd)
}

pub fn check_command() -> Command {
    Command::new("check")
        .about("Validate a query")
        .override_usage(
            "\
  plotnik check <PATH> --lang <LANG> [--strict]
  plotnik check - --lang <LANG> [--strict]",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik check query.ptk --lang javascript
  plotnik check queries/ --lang javascript
  plotnik check - --lang javascript

PATH is a PTK file or directory. '-' reads a named PTK module from stdin.
--lang selects the grammar. Tags group directory members by language.
Successful validation writes no stdout. Diagnostics go to stderr."#,
        )
        .arg(query_path_arg())
        .next_help_heading("Input options")
        .arg(lang_arg().required(true))
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
  plotnik dump <PATH> --lang <LANG>
  plotnik dump - --lang <LANG>",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik dump query.ptk --lang javascript
  plotnik dump - --lang javascript

PATH is a PTK file or directory. '-' reads a named PTK module from stdin.
--lang selects the grammar. Tags group directory members by language.
Prints the complete bytecode dump, including types, entrypoints, spans, and instructions."#,
        )
        .arg(query_path_arg())
        .next_help_heading("Input options")
        .arg(lang_arg().required(true))
        .arg(cache_dir_arg())
        .next_help_heading("Global options")
        .arg(color_arg())
}

fn execution_command(name: &'static str, about: &'static str) -> Command {
    let search_help = match name {
        "find" => {
            "\n\nSearch inclusively from the root, depth-first in source order. Return the first match."
        }
        "find-all" => {
            "\n\nSearch inclusively from the root, depth-first in source order. Skip descendants of matches.\n\
             Return a JSON array, or [] if nothing matches."
        }
        _ => "",
    };

    Command::new(name)
        .about(about)
        .override_usage(format!(
            "plotnik {name} [OPTIONS] --entry <NAME> <PATH> <FILE>\n  plotnik {name} [OPTIONS] - [PATH] <FILE>"
        ))
        .after_help(format!(
            "PATH is a PTK file or directory of .ptk files.\n\
             Use '-' to read a PTK expression from stdin. [PATH] supplies definitions it can use.\n\
             FILE is the source file. Its extension selects the grammar unless --lang is set.{search_help}",
        ))
        .arg(
            Arg::new("inputs")
                .value_name("INPUT")
                .value_parser(value_parser!(PathBuf))
                .num_args(1..=3)
                .action(clap::ArgAction::Append)
                .required(true)
                .help("PTK input followed by the source file"),
        )
        .next_help_heading("Input options")
        .arg(lang_arg())
        .arg(cache_dir_arg())
        .arg(entry_arg())
        .next_help_heading("Output options")
        .arg(compact_arg())
        .arg(
            Arg::new("trace")
                .long("trace")
                .action(clap::ArgAction::SetTrue)
                .help("Write the execution trace to stderr"),
        )
        .arg(verbose_arg().requires("trace"))
        .next_help_heading("Global options")
        .arg(color_arg())
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
        .after_help(
            r#"EXAMPLES:
  plotnik lang dump javascript
  plotnik lang dump owner/repo/grammar-directory@revision --no-legend --width 60"#,
        )
        .arg(cache_dir_arg())
        .arg(
            clap::Arg::new("lang")
                .value_name("LANG[@REVISION]")
                .help(LANGUAGE_HELP)
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
            clap::Arg::new("width")
                .long("width")
                .help("Fold groups inline up to this column width (0 = always break)")
                .value_parser(clap::value_parser!(usize)),
        )
}
