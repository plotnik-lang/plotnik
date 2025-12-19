use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

use plotnik_lib::QueryBuilder;

use super::debug::source::resolve_lang;

pub struct ExecArgs {
    pub query_text: Option<String>,
    pub query_file: Option<PathBuf>,
    pub source_text: Option<String>,
    pub source_file: Option<PathBuf>,
    pub lang: Option<String>,
    pub pretty: bool,
    pub verbose_nodes: bool,
    pub check: bool,
    pub entry: Option<String>,
}

pub fn run(args: ExecArgs) {
    if let Err(msg) = validate(&args) {
        eprintln!("error: {}", msg);
        std::process::exit(1);
    }

    let query_source = load_query(&args);
    if query_source.trim().is_empty() {
        eprintln!("error: query cannot be empty");
        std::process::exit(1);
    }
    let _source_code = load_source(&args);
    let lang = resolve_lang(&args.lang, &args.source_text, &args.source_file);

    // Parse query
    let query_parsed = QueryBuilder::new(&query_source)
        .parse()
        .unwrap_or_else(|e| {
            eprintln!("error: {}", e);
            std::process::exit(1);
        });

    // Analyze query
    let query_analyzed = query_parsed.analyze().unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        std::process::exit(1);
    });

    // Link query against language
    let linked = query_analyzed.link(&lang);
    if !linked.is_valid() {
        eprint!("{}", linked.diagnostics().render(&query_source));
        std::process::exit(1);
    }

    let _ = (args.pretty, args.verbose_nodes, args.check, args.entry);

    todo!("IR emission and query execution not yet implemented")
}

fn load_query(args: &ExecArgs) -> String {
    if let Some(ref text) = args.query_text {
        return text.clone();
    }
    if let Some(ref path) = args.query_file {
        if path.as_os_str() == "-" {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .expect("failed to read stdin");
            return buf;
        }
        return fs::read_to_string(path).unwrap_or_else(|_| {
            eprintln!("error: query file not found: {}", path.display());
            std::process::exit(1);
        });
    }
    unreachable!("validation ensures query input exists")
}

fn load_source(args: &ExecArgs) -> String {
    if let Some(ref text) = args.source_text {
        return text.clone();
    }
    if let Some(ref path) = args.source_file {
        if path.as_os_str() == "-" {
            panic!("cannot read both query and source from stdin");
        }
        return fs::read_to_string(path).unwrap_or_else(|_| {
            eprintln!("error: file not found: {}", path.display());
            std::process::exit(1);
        });
    }
    unreachable!("validation ensures source input exists")
}

fn validate(args: &ExecArgs) -> Result<(), &'static str> {
    let has_query = args.query_text.is_some() || args.query_file.is_some();
    let has_source = args.source_text.is_some() || args.source_file.is_some();

    if !has_query {
        return Err("query is required: use -q/--query or --query-file");
    }

    if !has_source {
        return Err("source is required: use -s/--source-file or --source");
    }

    if args.source_text.is_some() && args.lang.is_none() {
        return Err("--lang is required when using --source");
    }

    Ok(())
}
