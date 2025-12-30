#![allow(dead_code)]
pub mod source;

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
    pub symbols: bool,
    pub raw: bool,
    pub cst: bool,
    pub spans: bool,
    pub arities: bool,
    pub graph: bool,
    pub graph_raw: bool,
    pub types: bool,
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
        Query::try_from(src.as_str()).unwrap_or_else(|e| {
            eprintln!("error: {}", e);
            std::process::exit(1);
        })
    });

    let show_query = has_query_input && !args.symbols && !args.graph && !args.types;
    let show_source = has_source_input;

    if show_query && let Some(ref q) = query {
        print!(
            "{}",
            q.printer()
                .raw(args.cst || args.raw)
                .with_trivia(args.raw)
                .with_spans(args.spans)
                .with_arities(args.arities)
                .dump()
        );
    }

    if args.symbols
        && let Some(ref q) = query
    {
        print!(
            "{}",
            q.printer()
                .only_symbols(true)
                .with_arities(args.arities)
                .dump()
        );
    }

    if args.graph || args.graph_raw {
        eprintln!("error: --graph and --graph-raw are not yet implemented");
        std::process::exit(1);
    }

    if args.types
        && let Some(ref q) = query
    {
        let bytecode = q.emit().expect("bytecode emission failed");
        let module =
            plotnik_lib::bytecode::Module::from_bytes(bytecode).expect("module loading failed");
        let output = plotnik_lib::bytecode::emit::emit_typescript(&module);
        print!("{}", output);
    }

    if show_source {
        if show_query || args.symbols {
            println!();
        }
        let resolved_lang = resolve_lang(&args.lang, &args.source_text, &args.source_file);
        let source_code = load_source(&args.source_text, &args.source_file);
        let tree = parse_tree(&source_code, resolved_lang);
        print!("{}", dump_source(&tree, &source_code, args.raw));
    }

    if let Some(ref q) = query
        && !q.is_valid()
    {
        eprint!(
            "{}",
            q.diagnostics().render_colored(q.source_map(), args.color)
        );
        std::process::exit(1);
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
    if !has_query && !has_source {
        return Err(
            "specify at least one input: -q/--query, --query-file, -s/--source-file, or --source",
        );
    }

    if args.symbols && !has_query {
        return Err("--only-symbols requires -q/--query or --query-file");
    }

    if args.source_text.is_some() && args.lang.is_none() {
        return Err("--lang is required when using --source");
    }

    Ok(())
}

fn resolve_lang_for_link(lang: &Option<String>) -> plotnik_langs::Lang {
    let name = lang.as_ref().expect("--lang required for --link");
    plotnik_langs::from_name(name).unwrap_or_else(|| {
        eprintln!("error: unknown language: {}", name);
        std::process::exit(1);
    })
}
