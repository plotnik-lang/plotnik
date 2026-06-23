use std::path::PathBuf;

use plotnik_lib::QueryBuilder;

use super::lang_resolver::require_lang;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult};

pub struct CheckArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub strict: bool,
    pub json: bool,
    pub color: bool,
}

pub fn run(args: CheckArgs) -> CliResult {
    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    let query = QueryBuilder::new(loaded.sources)
        .analyze()
        .map_err(|e| CliError::fatal(e.to_string()))?;

    let lang = require_lang(
        args.lang.as_deref(),
        loaded.shebang.lang.as_deref(),
        "check",
    )?;
    let linked = query.link(lang.grammar());

    let diagnostics = linked.check_compile();
    let source_map = linked.source_map();
    let valid = if args.strict {
        !diagnostics.has_errors() && !diagnostics.has_warnings()
    } else {
        !diagnostics.has_errors()
    };

    if args.json {
        // Contract: on exit 0/1 stdout is a JSON array, `[]` when clean.
        // Exit 2 (couldn't answer) keeps text on stderr and emits no JSON.
        println!("{}", diagnostics.render_json(source_map));
    } else if !valid {
        eprint!("{}", diagnostics.render_colored(source_map, args.color));
    }

    if !valid {
        return Err(CliError::No);
    }

    // Silent on success (like cargo check)
    Ok(())
}
