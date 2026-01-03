mod cli;
mod commands;

use cli::{Cli, Command};
use commands::check::CheckArgs;
use commands::dump::DumpArgs;
use commands::exec::ExecArgs;
use commands::infer::InferArgs;
use commands::trace::TraceArgs;
use commands::tree::TreeArgs;

fn main() {
    let cli = <Cli as clap::Parser>::parse();

    match cli.command {
        Command::Tree {
            source_path,
            source_text,
            lang,
            raw,
            spans,
        } => {
            commands::tree::run(TreeArgs {
                source_path,
                source_text,
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
                void_type: infer_output.void_type,
            });
        }
        Command::Exec {
            query_path,
            source_path,
            query_text,
            source_text,
            lang,
            exec_output,
            output,
        } => {
            // Pretty by default when stdout is a TTY, unless --compact is passed
            let pretty =
                !exec_output.compact && std::io::IsTerminal::is_terminal(&std::io::stdout());
            commands::exec::run(ExecArgs {
                query_path,
                query_text,
                source_path,
                source_text,
                lang,
                pretty,
                entry: exec_output.entry,
                color: output.color.should_colorize(),
            });
        }
        Command::Trace {
            query_path,
            source_path,
            query_text,
            source_text,
            lang,
            entry,
            verbose,
            no_result,
            fuel,
            output,
        } => {
            use plotnik_lib::engine::Verbosity;

            let verbosity = match verbose {
                0 => Verbosity::Default,
                1 => Verbosity::Verbose,
                _ => Verbosity::VeryVerbose,
            };
            commands::trace::run(TraceArgs {
                query_path,
                query_text,
                source_path,
                source_text,
                lang,
                entry,
                verbosity,
                no_result,
                fuel,
                color: output.color.should_colorize(),
            });
        }
        Command::Langs => {
            commands::langs::run();
        }
    }
}
