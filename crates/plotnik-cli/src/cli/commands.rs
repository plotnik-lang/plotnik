//! Command builders for the CLI.
//!
//! Each command is built using the shared arg builders from `args.rs`.
//! The unified flags feature is implemented here: dump/exec/trace accept
//! all runtime flags, with irrelevant ones hidden from `--help`.

use clap::Command;

use super::args::*;

/// Add hidden source input args (for commands that don't use source).
fn with_hidden_source_args(cmd: Command) -> Command {
    cmd.arg(source_path_arg().hide(true))
        .arg(source_text_arg().hide(true))
}

/// Add hidden exec output args (for commands that don't produce JSON).
fn with_hidden_exec_args(cmd: Command) -> Command {
    cmd.arg(entry_arg().hide(true))
        .arg(compact_arg().hide(true))
        .arg(verbose_nodes_arg().hide(true))
        .arg(check_arg().hide(true))
}

/// Add hidden exec output args, excluding --verbose-nodes (for infer which has it visible).
fn with_hidden_exec_args_partial(cmd: Command) -> Command {
    cmd.arg(entry_arg().hide(true))
        .arg(compact_arg().hide(true))
        .arg(check_arg().hide(true))
}

/// Add hidden trace args (for commands that don't trace).
fn with_hidden_trace_args(cmd: Command) -> Command {
    cmd.arg(verbose_arg().hide(true))
        .arg(no_result_arg().hide(true))
        .arg(fuel_arg().hide(true))
}

/// Add hidden AST args (for commands that don't show AST).
fn with_hidden_ast_args(cmd: Command) -> Command {
    cmd.arg(raw_arg().hide(true))
}

/// Build the complete CLI with all subcommands.
pub fn build_cli() -> Command {
    Command::new("plotnik")
        .about("Query language for tree-sitter AST with type inference")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(ast_command())
        .subcommand(check_command())
        .subcommand(dump_command())
        .subcommand(infer_command())
        .subcommand(exec_command())
        .subcommand(trace_command())
        .subcommand(lang_command())
}

/// Show AST of query and/or source file.
///
/// Accepts all runtime flags for unified CLI experience.
/// Shows query AST when query is provided, source AST when source is provided.
pub fn ast_command() -> Command {
    let cmd = Command::new("ast")
        .about("Show AST of query and/or source file")
        .override_usage(
            "\
  plotnik ast <QUERY> [SOURCE]
  plotnik ast -q <TEXT> [SOURCE]
  plotnik ast <SOURCE>
  plotnik ast -s <TEXT> -l <LANG>",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik ast query.ptk               # query AST
  plotnik ast app.ts                  # source AST (tree-sitter)
  plotnik ast query.ptk app.ts        # both ASTs
  plotnik ast query.ptk app.ts --raw  # CST / include anonymous nodes
  plotnik ast -q '(id) @x'            # inline query AST
  plotnik ast -s 'let x = 1' -l js    # inline source AST"#,
        )
        .arg(query_path_arg())
        .arg(source_path_arg())
        .arg(query_text_arg())
        .arg(source_text_arg())
        .arg(lang_arg())
        .arg(raw_arg())
        .arg(color_arg());

    // Hidden unified flags
    with_hidden_trace_args(with_hidden_exec_args(cmd))
}

/// Validate a query.
///
/// Accepts all runtime flags for unified CLI experience, but only uses
/// query/lang/strict/color.
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
  plotnik check -q 'Q = ...' -l js    # inline query"#,
        )
        .arg(query_path_arg())
        .arg(query_text_arg())
        .arg(lang_arg())
        .arg(strict_arg())
        .arg(color_arg());

    // Hidden unified flags
    with_hidden_ast_args(with_hidden_trace_args(with_hidden_exec_args(
        with_hidden_source_args(cmd),
    )))
}

/// Show compiled bytecode.
///
/// Accepts all runtime flags for unified CLI experience, but only uses
/// query/lang/color. Source and execution flags are hidden and ignored.
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
  plotnik dump query.ptk             # unlinked bytecode
  plotnik dump query.ptk -l ts       # linked (resolved node types)
  plotnik dump -q 'Q = ...'          # inline query"#,
        )
        .arg(query_path_arg())
        .arg(query_text_arg())
        .arg(lang_arg())
        .arg(color_arg());

    // Hidden unified flags
    with_hidden_ast_args(with_hidden_trace_args(with_hidden_exec_args(
        with_hidden_source_args(cmd),
    )))
}

/// Generate type definitions from a query.
///
/// Accepts all runtime flags for unified CLI experience, but only uses
/// query/lang and infer-specific options.
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
  plotnik infer query.ptk -l js -o types.d.ts  # write to file

NOTE: Use --verbose-nodes to match `exec --verbose-nodes` output shape."#,
        )
        .arg(query_path_arg())
        .arg(query_text_arg())
        .arg(lang_arg())
        .arg(format_arg())
        .arg(verbose_nodes_arg())
        .arg(no_node_type_arg())
        .arg(no_export_arg())
        .arg(void_type_arg())
        .arg(output_file_arg())
        .arg(color_arg());

    // Hidden unified flags (use partial exec args since --verbose-nodes is visible)
    with_hidden_ast_args(with_hidden_trace_args(with_hidden_exec_args_partial(
        with_hidden_source_args(cmd),
    )))
}

/// Execute a query against source code and output JSON.
///
/// Accepts trace flags for unified CLI experience, but ignores them.
pub fn exec_command() -> Command {
    let cmd = Command::new("exec")
        .about("Execute a query against source code and output JSON")
        .override_usage(
            "\
  plotnik exec <QUERY> <SOURCE>
  plotnik exec -q <TEXT> <SOURCE>
  plotnik exec -q <TEXT> -s <TEXT> -l <LANG>",
        )
        .after_help(
            r#"EXAMPLES:
  plotnik exec query.ptk app.js           # two positional files
  plotnik exec -q 'Q = ...' app.js        # inline query + source file
  plotnik exec -q 'Q = ...' -s 'let x' -l js  # all inline"#,
        )
        .arg(query_path_arg())
        .arg(source_path_arg())
        .arg(query_text_arg())
        .arg(source_text_arg())
        .arg(lang_arg())
        .arg(color_arg())
        .arg(compact_arg())
        .arg(verbose_nodes_arg())
        .arg(check_arg())
        .arg(entry_arg());

    // Hidden unified flags
    with_hidden_ast_args(with_hidden_trace_args(cmd))
}

/// Trace query execution for debugging.
///
/// Accepts exec output flags for unified CLI experience, but ignores them.
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
        .arg(query_text_arg())
        .arg(source_text_arg())
        .arg(lang_arg())
        .arg(color_arg())
        .arg(entry_arg())
        .arg(verbose_arg())
        .arg(no_result_arg())
        .arg(fuel_arg());

    // Hidden unified flags (exec output flags only - entry is visible for trace)
    with_hidden_ast_args(
        cmd.arg(compact_arg().hide(true))
            .arg(verbose_nodes_arg().hide(true))
            .arg(check_arg().hide(true)),
    )
}

/// Language information commands.
pub fn lang_command() -> Command {
    Command::new("lang")
        .about("Language information and grammar dump")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(lang_list_command())
        .subcommand(lang_dump_command())
}

/// List supported languages.
fn lang_list_command() -> Command {
    Command::new("list").about("List supported languages with aliases")
}

/// Dump grammar for a language.
fn lang_dump_command() -> Command {
    Command::new("dump")
        .about("Dump grammar in Plotnik-like syntax")
        .arg(
            clap::Arg::new("lang")
                .help("Language name or alias")
                .required(true)
                .index(1),
        )
}
