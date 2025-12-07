mod cli;
mod commands;

use cli::{Cli, Command};
use commands::debug::DebugArgs;
use commands::infer::InferArgs;

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
                color: output.color.should_colorize(),
            });
        }
        Command::Infer {
            query,
            lang,
            common,
            rust,
            typescript,
        } => {
            commands::infer::run(InferArgs {
                query_text: query.query_text,
                query_file: query.query_file,
                lang,
                entry_name: common.entry_name,
                color: common.color.should_colorize(),
                indirection: rust.indirection,
                derive: rust.derive,
                no_derive: rust.no_derive,
                optional: typescript.optional,
                export: typescript.export,
                readonly: typescript.readonly,
                type_alias: typescript.type_alias,
                node_type: typescript.node_type,
                nested: typescript.nested,
            });
        }
        Command::Docs { topic } => {
            commands::docs::run(topic.as_deref());
        }
        Command::Langs => {
            commands::langs::run();
        }
    }
}
