use std::path::PathBuf;

use plotnik_lib::QueryBuilder;

use super::lang_resolver::require_lang;
use super::query_loader::load_query;
use crate::error::{CliError, CliResult, write_stderr, writeln_stdout};

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

    let lang = require_lang(
        args.lang.as_deref(),
        loaded.shebang.lang.as_deref(),
        "check",
    )?;
    let checked = QueryBuilder::new(loaded.sources)
        .with_strict_lints(args.strict)
        .compile(lang.grammar())
        .map_err(|e| CliError::fatal(e.to_string()))?;

    let diagnostics = checked.diagnostics();
    let source_map = checked.source_map();
    let valid = if args.strict {
        !diagnostics.has_errors() && !diagnostics.has_warnings()
    } else {
        !diagnostics.has_errors()
    };

    if args.json {
        // Contract: on exit 0/1 stdout is a JSON array, `[]` when clean.
        // Exit 2 (couldn't answer) keeps text on stderr and emits no JSON.
        writeln_stdout(format_args!("{}", diagnostics.render_json(source_map)))?;
    } else if !valid || diagnostics.has_warnings() {
        // Warnings print even when the query is valid (like cargo check);
        // only a fully clean query stays silent.
        write_stderr(format_args!(
            "{}",
            diagnostics.render_colored(source_map, args.color)
        ))?;
    }

    if !valid {
        return Err(CliError::No);
    }

    Ok(())
}
