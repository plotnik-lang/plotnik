mod cli;
mod commands;

use cli::{Cli, Command};
use commands::debug::DebugArgs;
use commands::exec::ExecArgs;
use commands::types::TypesArgs;

fn main() {
    let cli = <Cli as clap::Parser>::parse();

    match cli.command {
        Command::Debug {
            query,
            source,
            lang,
            output,
        } => {
            commands::debug::run(DebugArgs {
                query_text: query.query_text,
                query_file: query.query_file,
                source_text: source.source_text,
                source_file: source.source_file,
                lang,
                symbols: output.symbols,
                raw: output.raw,
                cst: output.cst,
                spans: output.spans,
                cardinalities: output.cardinalities,
                graph: output.graph,
                graph_raw: output.graph_raw,
                types: output.types,
                color: output.color.should_colorize(),
            });
        }
        Command::Docs { topic } => {
            commands::docs::run(topic.as_deref());
        }
        Command::Langs => {
            commands::langs::run();
        }
        Command::Exec {
            query,
            source,
            lang,
            output,
        } => {
            commands::exec::run(ExecArgs {
                query_text: query.query_text,
                query_file: query.query_file,
                source_text: source.source_text,
                source_file: source.source_file,
                lang,
                entry: output.entry,
                pretty: output.pretty,
                verbose_nodes: output.verbose_nodes,
                check: output.check,
            });
        }
        Command::Types {
            query,
            lang,
            output,
        } => {
            commands::types::run(TypesArgs {
                query_text: query.query_text,
                query_file: query.query_file,
                lang,
                format: output.format,
                root_type: output.root_type,
                verbose_nodes: output.verbose_nodes,
                no_node_type: output.no_node_type,
                export: !output.no_export,
                output: output.output,
            });
        }
    }
}
