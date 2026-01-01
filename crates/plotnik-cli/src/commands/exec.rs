use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

use plotnik_langs::Lang;
use plotnik_lib::QueryBuilder;

use super::lang_resolver::{resolve_lang_required, suggest_language};
use super::query_loader::load_query_source;

pub struct ExecArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub pretty: bool,
    pub verbose_nodes: bool,
    pub check: bool,
    pub entry: Option<String>,
    pub color: bool,
}

pub fn run(args: ExecArgs) {
    if let Err(msg) = validate(&args) {
        eprintln!("error: {}", msg);
        std::process::exit(1);
    }

    let source_map = match load_query_source(
        args.query_path.as_deref(),
        args.query_text.as_deref(),
    ) {
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

    let _source_code = load_source(&args);
    let lang = resolve_source_lang(&args);

    // Parse and analyze query
    let query = match QueryBuilder::new(source_map).parse() {
        Ok(parsed) => parsed.analyze().link(&lang),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };

    if !query.is_valid() {
        eprint!(
            "{}",
            query.diagnostics().render_colored(query.source_map(), args.color)
        );
        std::process::exit(1);
    }

    let _ = (args.pretty, args.verbose_nodes, args.check, args.entry);

    eprintln!("The 'exec' command is under development.");
    eprintln!();
    eprintln!("For now, use 'plotnik infer' to generate TypeScript types.");
    std::process::exit(0);
}

fn load_source(args: &ExecArgs) -> String {
    if let Some(ref text) = args.source_text {
        return text.clone();
    }
    if let Some(ref path) = args.source_path {
        if path.as_os_str() == "-" {
            // Check if query is also from stdin
            if args.query_path.as_ref().map(|p| p.as_os_str() == "-").unwrap_or(false) {
                eprintln!("error: query and source cannot both be from stdin");
                std::process::exit(1);
            }
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .expect("failed to read stdin");
            return buf;
        }
        return fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("error: failed to read '{}': {}", path.display(), e);
            std::process::exit(1);
        });
    }
    unreachable!("validation ensures source input exists")
}

fn resolve_source_lang(args: &ExecArgs) -> Lang {
    if let Some(ref name) = args.lang {
        return resolve_lang_required(name).unwrap_or_else(|msg| {
            eprintln!("error: {}", msg);
            if let Some(suggestion) = suggest_language(name) {
                eprintln!();
                eprintln!("Did you mean '{}'?", suggestion);
            }
            eprintln!();
            eprintln!("Run 'plotnik langs' for the full list.");
            std::process::exit(1);
        });
    }

    if let Some(ref path) = args.source_path
        && path.as_os_str() != "-"
        && let Some(ext) = path.extension().and_then(|e| e.to_str())
    {
        if let Some(lang) = plotnik_langs::from_ext(ext) {
            return lang;
        }
        eprintln!(
            "error: cannot infer language from extension '.{}', use --lang",
            ext
        );
        std::process::exit(1);
    }

    eprintln!("error: --lang is required (cannot infer from input)");
    std::process::exit(1);
}

fn validate(args: &ExecArgs) -> Result<(), &'static str> {
    let has_query = args.query_text.is_some() || args.query_path.is_some();
    let has_source = args.source_text.is_some() || args.source_path.is_some();

    if !has_query {
        return Err("query is required: use positional argument, -q/--query, or --query-file");
    }

    if !has_source {
        return Err("source is required: use positional argument, -s/--source-file, or --source");
    }

    if args.source_text.is_some() && args.lang.is_none() {
        return Err("--lang is required when using --source");
    }

    Ok(())
}
