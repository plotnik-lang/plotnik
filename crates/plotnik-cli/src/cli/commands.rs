//! Command builders for the CLI.
//!
//! Each command is built using the shared arg builders from `args.rs`.
//! The unified flags feature is implemented here: dump/exec/trace accept
//! all runtime flags, with irrelevant ones hidden from `--help`.

use clap::Command;

use super::args::*;
use super::limits::{limits_preset_arg, max_memory_arg, max_steps_arg};

fn with_hidden_source_args(cmd: Command) -> Command {
    cmd.arg(source_path_arg().hide(true))
        .arg(source_text_arg().hide(true))
}

fn with_hidden_exec_args(cmd: Command) -> Command {
    cmd.arg(entry_arg().hide(true))
        .arg(compact_arg().hide(true))
        .arg(verbose_nodes_arg().hide(true))
}

// --verbose-nodes is visible for infer, so exclude it from the hidden set.
fn with_hidden_exec_args_partial(cmd: Command) -> Command {
    cmd.arg(entry_arg().hide(true))
        .arg(compact_arg().hide(true))
}

fn with_hidden_trace_args(cmd: Command) -> Command {
    cmd.arg(verbose_arg().hide(true))
        .arg(no_result_arg().hide(true))
}

// Runtime-limit flags, hidden. Added to non-exec commands so the unified flag
// set parses them without error; only `run`/`trace` surface them visibly.
fn with_hidden_runtime_limit_args(cmd: Command) -> Command {
    cmd.arg(max_steps_arg().hide(true))
        .arg(max_memory_arg().hide(true))
        .arg(limits_preset_arg().hide(true))
}

fn with_hidden_ast_args(cmd: Command) -> Command {
    cmd.arg(raw_arg().hide(true))
}

fn with_hidden_json_arg(cmd: Command) -> Command {
    cmd.arg(json_arg().hide(true))
}

pub fn build_cli() -> Command {
    Command::new("plotnik")
        .about("Query language for tree-sitter AST with type inference")
        .version(env!("CARGO_PKG_VERSION"))
        .long_version(format!(
            "{} ({} bundled languages)",
            env!("CARGO_PKG_VERSION"),
            crate::language_registry::all().len()
        ))
        .propagate_version(true)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .after_help(
            r#"EXIT CODES:
  0  yes/success (match found, query valid)
  1  no (run: no match; check: invalid)
  2  couldn't answer (usage, IO, or internal error)

Run 'plotnik <command> --help' for examples."#,
        )
        .subcommand(run_command())
        .subcommand(check_command())
        .subcommand(ast_command())
        .subcommand(infer_command())
        .subcommand(dump_command())
        .subcommand(trace_command())
        .subcommand(lang_command())
        .subcommand(completions_command())
}

pub fn ast_command() -> Command {
    let cmd = Command::new("ast")
        .about("Show AST of query and/or source file")
        .override_usage(
            "\
  plotnik ast <FILE>                 # auto-detect by extension
  plotnik ast <QUERY> <SOURCE>       # both ASTs
  plotnik ast -q <TEXT> [SOURCE]
  plotnik ast -s <TEXT> -l <LANG>",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik ast query.ptk               # query AST (.ptk extension)
  plotnik ast app.ts                  # source AST (tree-sitter)
  plotnik ast query.ptk app.ts        # both ASTs
  plotnik ast query.ptk app.ts --raw  # CST / include anonymous nodes
  plotnik ast -q '(id) @x'            # inline query AST
  plotnik ast -s 'let x = 1' -l js    # inline source AST"#,
        )
        .arg(query_path_arg())
        .arg(source_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(source_text_arg())
        .arg(lang_arg())
        .next_help_heading("Output options")
        .arg(raw_arg())
        .next_help_heading("Global options")
        .arg(color_arg());

    with_hidden_json_arg(with_hidden_runtime_limit_args(with_hidden_trace_args(
        with_hidden_exec_args(cmd),
    )))
}

pub fn check_command() -> Command {
    let cmd = Command::new("check")
        .about("Validate a query")
        .override_usage(
            "\
  plotnik check <QUERY>
  plotnik check <QUERY> -l <LANG>
  plotnik check -q <TEXT> [-l <LANG>]",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik check query.ptk             # validate syntax only
  plotnik check query.ptk -l ts       # also check against grammar
  plotnik check queries.ts/           # workspace directory
  plotnik check -q 'Q = ...' -l js    # inline query
  plotnik check query.ptk --json      # diagnostics as JSON"#,
        )
        .arg(query_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(lang_arg())
        .next_help_heading("Check options")
        .arg(strict_arg())
        .arg(json_arg())
        .next_help_heading("Global options")
        .arg(color_arg());

    with_hidden_runtime_limit_args(with_hidden_ast_args(with_hidden_trace_args(
        with_hidden_exec_args(with_hidden_source_args(cmd)),
    )))
}

pub fn dump_command() -> Command {
    let cmd = Command::new("dump")
        .about("Show compiled bytecode")
        .override_usage(
            "\
  plotnik dump <QUERY>
  plotnik dump <QUERY> -l <LANG>
  plotnik dump -q <TEXT> [-l <LANG>]",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik dump query.ptk -l ts       # resolved node kinds
  plotnik dump -q 'Q = ...' -l ts    # inline query"#,
        )
        .arg(query_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(lang_arg())
        .next_help_heading("Global options")
        .arg(color_arg());

    with_hidden_json_arg(with_hidden_runtime_limit_args(with_hidden_ast_args(
        with_hidden_trace_args(with_hidden_exec_args(with_hidden_source_args(cmd))),
    )))
}

pub fn infer_command() -> Command {
    let cmd = Command::new("infer")
        .about("Generate type definitions from a query")
        .override_usage(
            "\
  plotnik infer <QUERY> -l <LANG>
  plotnik infer -q <TEXT> -l <LANG>",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik infer query.ptk -l js       # from file
  plotnik infer -q 'Q = ...' -l ts    # inline query
  plotnik infer query.ptk -l js -o types.d.ts  # write to file"#,
        )
        .arg(query_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(lang_arg())
        .next_help_heading("Output options")
        .arg(format_arg())
        .arg(verbose_nodes_arg())
        .arg(no_node_type_arg())
        .arg(no_export_arg())
        .arg(void_type_arg())
        .arg(output_file_arg())
        .next_help_heading("Global options")
        .arg(color_arg());

    with_hidden_json_arg(with_hidden_runtime_limit_args(with_hidden_ast_args(
        with_hidden_trace_args(with_hidden_exec_args_partial(with_hidden_source_args(cmd))),
    )))
}

pub fn run_command() -> Command {
    let cmd = Command::new("run")
        .alias("exec")
        .about("Execute a query against source code and output JSON")
        .override_usage(
            "\
  plotnik run <QUERY> <SOURCE>
  plotnik run -q <TEXT> <SOURCE>
  plotnik run -q <TEXT> -s <TEXT> -l <LANG>",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik run query.ptk app.js           # two positional files
  plotnik run -q 'Q = ...' app.js        # inline query + source file
  plotnik run -q 'Q = ...' -s 'let x' -l js  # all inline"#,
        )
        .arg(query_path_arg())
        .arg(source_path_arg())
        .next_help_heading("Input options")
        .arg(query_text_arg())
        .arg(source_text_arg())
        .arg(lang_arg())
        .arg(entry_arg())
        .next_help_heading("Output options")
        .arg(compact_arg())
        .arg(verbose_nodes_arg().hide(true))
        .next_help_heading("Limit options")
        .arg(max_steps_arg())
        .arg(max_memory_arg())
        .arg(limits_preset_arg())
        .next_help_heading("Global options")
        .arg(color_arg());

    with_hidden_json_arg(with_hidden_ast_args(with_hidden_trace_args(cmd)))
}

pub fn trace_command() -> Command {
    let cmd = Command::new("trace")
        .about("Trace query execution for debugging")
        .override_usage(
            "\
  plotnik trace <QUERY> <SOURCE>
  plotnik trace -q <TEXT> <SOURCE>
  plotnik trace -q <TEXT> -s <TEXT> -l <LANG>",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik trace query.ptk app.js          # two positional files
  plotnik trace -q 'Q = ...' app.js       # inline query + source file
  plotnik trace -q 'Q = ...' -s 'let x' -l js  # all inline"#,
        )
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
        .arg(max_steps_arg())
        .arg(max_memory_arg())
        .arg(limits_preset_arg())
        .next_help_heading("Global options")
        .arg(color_arg());

    with_hidden_json_arg(with_hidden_ast_args(
        cmd.arg(compact_arg().hide(true))
            .arg(verbose_nodes_arg().hide(true)),
    ))
}

pub fn lang_command() -> Command {
    Command::new("lang")
        .about("Language information and grammar dump")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .flatten_help(true)
        .subcommand(lang_list_command())
        .subcommand(lang_dump_command())
}

fn lang_list_command() -> Command {
    Command::new("list").about("List supported languages with aliases")
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
        .about("Dump grammar tree shapes in query-flavored notation")
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
