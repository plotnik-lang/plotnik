use std::path::PathBuf;

use plotnik_lib::Colors;
use plotnik_lib::QueryBuilder;
use plotnik_lib::bytecode::{Module, dump};

use super::lang_resolver::require_lang;
use super::query_loader::load_query_source;
use crate::error::{CliError, CliResult};

pub struct DumpArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub color: bool,
}

pub fn run(args: DumpArgs) -> CliResult {
    let loaded = load_query_source(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    // Parse and analyze
    let query = QueryBuilder::new(loaded.sources)
        .parse()
        .map_err(|e| CliError::fatal(e.to_string()))?
        .analyze();

    let lang = require_lang(
        args.lang.as_deref(),
        loaded.shebang.lang.as_deref(),
        args.query_path.as_deref(),
        "dump",
    )?;

    let linked = query.link(lang.grammar());
    if !linked.is_valid() {
        eprint!(
            "{}",
            linked
                .diagnostics()
                .render_colored(linked.source_map(), args.color)
        );
        return Err(CliError::FatalRendered);
    }
    let bytecode = linked.emit().map_err(|e| CliError::fatal(e.to_string()))?;

    let module = Module::load(&bytecode).expect("module loading failed");
    let colors = Colors::new(args.color);
    print!("{}", dump(&module, colors));

    Ok(())
}
