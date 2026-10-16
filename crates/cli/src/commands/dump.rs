use std::path::PathBuf;

use plotnik_compiler::Colors;
use plotnik_compiler::bytecode::dump;

use super::compile::compile_module;
use super::language::LanguageOptions;
use super::query_loader::load_query;
use crate::error::{CliResult, write_stdout};

pub struct DumpArgs {
    pub query_path: PathBuf,
    pub language: LanguageOptions,
    pub color: bool,
}

pub fn run(args: DumpArgs) -> CliResult {
    let sources = load_query(&args.query_path)?.into_collection();
    let artifacts = args.language.load(None)?;
    let module = compile_module(sources, &artifacts.grammar, args.color)?;
    let colors = Colors::new(args.color);
    write_stdout(format_args!("{}", dump(&module, colors)))?;

    Ok(())
}
