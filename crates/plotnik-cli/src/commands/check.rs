use std::path::PathBuf;

use plotnik_lib::QueryBuilder;

use super::lang_resolver::{resolve_lang, resolve_lang_required, suggest_language};
use super::query_loader::load_query_source;

pub struct CheckArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub strict: bool,
    pub color: bool,
}

pub fn run(args: CheckArgs) {
    let source_map = match load_query_source(args.query_path.as_deref(), args.query_text.as_deref())
    {
        Ok(map) => map,
        Err(msg) => {
            eprintln!("error: {}", msg);
            std::process::exit(1);
        }
    };

    if source_map.is_empty() {
        eprintln!("error: query cannot be empty");
        std::process::exit(1);
    }

    // Parse and analyze
    let query = match QueryBuilder::new(source_map).parse() {
        Ok(parsed) => parsed.analyze(),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };

    // Resolve language: explicit flag takes precedence, then infer from workspace
    let lang = match &args.lang {
        Some(name) => Some(resolve_lang_required(name).unwrap_or_else(|msg| {
            eprintln!("error: {}", msg);
            if let Some(suggestion) = suggest_language(name) {
                eprintln!();
                eprintln!("Did you mean '{}'?", suggestion);
            }
            eprintln!();
            eprintln!("Run 'plotnik langs' for the full list.");
            std::process::exit(1);
        })),
        None => resolve_lang(None, args.query_path.as_deref()),
    };

    let (is_valid, diagnostics, source_map) = match lang {
        Some(lang) => {
            let linked = query.link(&lang);
            let valid = if args.strict {
                !linked.diagnostics().has_errors() && !linked.diagnostics().has_warnings()
            } else {
                linked.is_valid()
            };
            (valid, linked.diagnostics(), linked.source_map().clone())
        }
        None => {
            let valid = if args.strict {
                !query.diagnostics().has_errors() && !query.diagnostics().has_warnings()
            } else {
                query.is_valid()
            };
            (valid, query.diagnostics(), query.source_map().clone())
        }
    };

    if !is_valid {
        eprint!("{}", diagnostics.render_colored(&source_map, args.color));
        std::process::exit(1);
    }

    // Silent on success (like cargo check)
}
