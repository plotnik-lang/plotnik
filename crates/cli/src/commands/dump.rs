use std::path::PathBuf;

use plotnik_compiler::Colors;
use plotnik_compiler::bytecode::dump;

use super::compile::compile_module;
use super::language::LanguageOptions;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult, write_stdout};

pub struct DumpArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub language: LanguageOptions,
    pub color: bool,
}

pub fn run(args: DumpArgs) -> CliResult {
    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    let artifacts = args.language.load_query(&loaded, None, args.color)?;
    let module = compile_module(loaded.sources, &artifacts.grammar, args.color)?;
    let colors = Colors::new(args.color);
    write_stdout(format_args!("{}", dump(&module, colors)))?;

    Ok(())
}
