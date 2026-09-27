use std::path::PathBuf;

use plotnik_lib::Colors;
use plotnik_lib::bytecode::dump;

use super::compile::compile_module;
use super::grammar;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult, write_stdout};

pub struct DumpArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub grammar: PathBuf,
    pub color: bool,
}

pub fn run(args: DumpArgs) -> CliResult {
    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    let grammar = grammar::load(&args.grammar)?;
    let module = compile_module(loaded.sources, &grammar, args.color)?;
    let colors = Colors::new(args.color);
    write_stdout(format_args!("{}", dump(&module, colors)))?;

    Ok(())
}
