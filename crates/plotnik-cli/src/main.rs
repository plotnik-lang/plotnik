mod cli;
mod commands;

use cli::{Cli, Command};

fn main() {
    let cli = <Cli as clap::Parser>::parse();

    match cli.command {
        Command::Debug {
            query,
            source,
            lang,
            output,
        } => {
            commands::debug::run(query, source, lang, output);
        }
        Command::Docs { topic } => {
            commands::docs::run(topic.as_deref());
        }
        Command::Langs => {
            commands::langs::run();
        }
    }
}