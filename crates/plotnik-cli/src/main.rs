mod cli;
mod commands;

use cli::{Cli, Command};
use commands::check::CheckArgs;
use commands::dump::DumpArgs;
use commands::exec::ExecArgs;
use commands::infer::InferArgs;
use commands::tree::TreeArgs;

fn main() {
    let cli = <Cli as clap::Parser>::parse();

    match cli.command {
        Command::Tree {
            source,
            lang,
            raw,
            spans,
        } => {
            commands::tree::run(TreeArgs {
                source_path: source,
                lang,
                raw,
                spans,
            });
        }
        Command::Check {
            query_path,
            query_text,
            lang,
            strict,
            output,
        } => {
            commands::check::run(CheckArgs {
                query_path,
                query_text,
                lang,
                strict,
                color: output.color.should_colorize(),
            });
        }
        Command::Dump {
            query_path,
            query_text,
            lang,
            output,
        } => {
            commands::dump::run(DumpArgs {
                query_path,
                query_text,
                lang,
                color: output.color.should_colorize(),
            });
        }
        Command::Infer {
            query_path,
            query_text,
            lang,
            infer_output,
            output,
        } => {
            commands::infer::run(InferArgs {
                query_path,
                query_text,
                lang,
                format: infer_output.format,
                verbose_nodes: infer_output.verbose_nodes,
                no_node_type: infer_output.no_node_type,
                export: !infer_output.no_export,
                output: infer_output.output,
                color: output.color.should_colorize(),
            });
        }
        Command::Exec {
            query_path,
            source_path,
            query_text,
            source_text,
            source_file,
            lang,
            exec_output,
            output,
        } => {
            // Merge source_path and source_file (positional takes precedence)
            let resolved_source = source_path.or(source_file);
            commands::exec::run(ExecArgs {
                query_path,
                query_text,
                source_path: resolved_source,
                source_text,
                lang,
                pretty: exec_output.pretty,
                verbose_nodes: exec_output.verbose_nodes,
                check: exec_output.check,
                entry: exec_output.entry,
                color: output.color.should_colorize(),
            });
        }
        Command::Langs => {
            commands::langs::run();
        }
    }
}
