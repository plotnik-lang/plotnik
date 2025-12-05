mod source;

use std::fs;
use std::io::{self, Read};

use plotnik_lib::Query;

use source::{dump_source, load_source, parse_tree, resolve_lang};

pub struct DebugArgs {
    pub query_text: Option<String>,
    pub query_file: Option<std::path::PathBuf>,
    pub source_text: Option<String>,
    pub source_file: Option<std::path::PathBuf>,
    pub lang: Option<String>,
    pub query: bool,
    pub source: bool,
    pub symbols: bool,
    pub raw: bool,
    pub cst: bool,
    pub spans: bool,
    pub cardinalities: bool,
    pub color: bool,
}

pub fn run(args: DebugArgs) {
    let has_query_input = args.query_text.is_some() || args.query_file.is_some();
    let has_source_input = args.source_text.is_some() || args.source_file.is_some();

    if let Err(msg) = validate(&args, has_query_input, has_source_input) {
        eprintln!("error: {}", msg);
        std::process::exit(1);
    }

    let query_source = if has_query_input {
        Some(load_query(&args))
    } else {
        None
    };

    let query = query_source.as_ref().map(|src| {
        Query::new(src).unwrap_or_else(|e| {
            eprintln!("error: {}", e);
            std::process::exit(1);
        })
    });

    let show_headers = [args.query, args.source, args.symbols]
        .iter()
        .filter(|&&x| x)
        .count()
        >= 2;

    if args.query
        && let Some(ref q) = query
    {
        if show_headers {
            println!("=== QUERY ===");
        }
        print!(
            "{}",
            q.printer()
                .raw(args.cst || args.raw)
                .with_trivia(args.raw)
                .with_spans(args.spans)
                .with_cardinalities(args.cardinalities)
                .dump()
        );
    }

    if args.symbols
        && let Some(ref q) = query
    {
        if show_headers {
            println!("=== SYMBOLS ===");
        }
        print!(
            "{}",
            q.printer()
                .only_symbols(true)
                .with_cardinalities(args.cardinalities)
                .dump()
        );
    }

    if args.source {
        let resolved_lang = resolve_lang(&args.lang, &args.source_text, &args.source_file);
        let source_code = load_source(&args.source_text, &args.source_file);
        let tree = parse_tree(&source_code, resolved_lang);
        if show_headers {
            println!("=== SOURCE ===");
        }
        print!("{}", dump_source(&tree, &source_code, args.raw));
    }

    if let Some(ref q) = query
        && !q.is_valid()
    {
        let src = query_source.as_ref().unwrap();
        eprint!("{}", q.diagnostics().render_colored(src, args.color));
    }
}

fn load_query(args: &DebugArgs) -> String {
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
        return fs::read_to_string(path).expect("failed to read query file");
    }
    unreachable!()
}

fn validate(args: &DebugArgs, has_query: bool, has_source: bool) -> Result<(), &'static str> {
    if (args.query || args.symbols) && !has_query {
        return Err("--query and --only-symbols require --query-text or --query-file");
    }

    if args.source && !has_source {
        return Err("--source requires --source-text or --source-file");
    }

    if args.source_text.is_some() && args.lang.is_none() {
        return Err("--lang is required when using --source-text");
    }

    if !args.query && !args.source && !args.symbols {
        return Err("specify at least one output: --query, --source, or --only-symbols");
    }

    Ok(())
}
