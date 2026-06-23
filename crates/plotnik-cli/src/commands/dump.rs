use std::path::PathBuf;

use plotnik_lib::Colors;
use plotnik_lib::bytecode::dump;

use super::compile::compile_module;
use super::lang_resolver::require_lang;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult};

pub struct DumpArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub color: bool,
}

pub fn run(args: DumpArgs) -> CliResult {
    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    let lang = require_lang(args.lang.as_deref(), loaded.shebang.lang.as_deref(), "dump")?;

    let module = compile_module(loaded.sources, lang, args.color)?;
    let colors = Colors::new(args.color);
    print!("{}", dump(&module, colors));

    Ok(())
}
