use std::path::PathBuf;

use plotnik_lib::QueryBuilder;

use super::lang_resolver::{infer_lang_from_dir, merge_lang};
use super::query_loader::load_query_source;
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
    let loaded = load_query_source(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    let query = QueryBuilder::new(loaded.sources)
        .parse()
        .map_err(|e| CliError::fatal(e.to_string()))?
        .analyze();

    // Resolve language: explicit -l (must agree with shebang) > shebang > dir inference
    let lang = match merge_lang(args.lang.as_deref(), loaded.shebang.lang.as_deref())? {
        Some(lang) => Some(lang),
        None => infer_lang_from_dir(args.query_path.as_deref()),
    };

    let (is_valid, diagnostics, source_map) = match lang {
        Some(lang) => {
            let linked = query.link(lang.grammar());
            let valid = is_valid(linked.diagnostics(), args.strict);
            (valid, linked.diagnostics(), linked.source_map().clone())
        }
        None => {
            let valid = is_valid(query.diagnostics(), args.strict);
            (valid, query.diagnostics(), query.source_map().clone())
        }
    };

    if args.json {
        // Contract: on exit 0/1 stdout is a JSON array, `[]` when clean.
        // Exit 2 (couldn't answer) keeps text on stderr and emits no JSON.
        println!("{}", diagnostics.render_json(&source_map));
    } else if !is_valid {
        eprint!("{}", diagnostics.render_colored(&source_map, args.color));
    }

    if !is_valid {
        return Err(CliError::No);
    }

    // Silent on success (like cargo check)
    Ok(())
}

fn is_valid(diagnostics: plotnik_lib::Diagnostics, strict: bool) -> bool {
    if strict {
        !diagnostics.has_errors() && !diagnostics.has_warnings()
    } else {
        !diagnostics.has_errors()
    }
}
