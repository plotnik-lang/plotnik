mod cli;
mod commands;

use cli::{Cli, Command};
use commands::debug::DebugArgs;

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
                query: output.query,
                source: output.source,
                symbols: output.symbols,
                raw: output.raw,
                cst: output.cst,
                spans: output.spans,
                cardinalities: output.cardinalities,
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
