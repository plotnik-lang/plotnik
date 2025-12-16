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

    let mut query = query_source.as_ref().map(|src| {
        Query::try_from(src).unwrap_or_else(|e| {
            eprintln!("error: {}", e);
            std::process::exit(1);
        })
    });

    // Auto-link when --lang is provided with a query
    if args.lang.is_some()
        && let Some(ref mut q) = query
    {
        let lang = resolve_lang_for_link(&args.lang);
        q.link(&lang);
    }

    let show_query = has_query_input && !args.symbols && !args.graph && !args.types;
    let show_source = has_source_input;
    let show_both_graphs = args.graph_raw && args.graph;

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

    // Build graph if needed for --graph, --graph-raw, or --types
    if (args.graph || args.graph_raw || args.types)
        && let Some(q) = query.take()
    {
        // Determine root kind for auto-wrapping
        let root_kind = args.lang.as_ref().and_then(|lang_name| {
            let lang = resolve_lang_for_link(&Some(lang_name.clone()));
            lang.root().and_then(|root_id| lang.node_type_name(root_id))
        });

        let (q, pre_opt_dump) = q.build_graph_with_pre_opt_dump(root_kind);
        let mut needs_separator = false;
        if args.graph_raw {
            if show_both_graphs {
                println!("(pre-optimization)");
            }
            print!("{}", pre_opt_dump);
            needs_separator = true;
        }
        if args.graph {
            if needs_separator {
                println!();
            }
            if show_both_graphs {
                println!("(post-optimization)");
            }
            print!("{}", q.graph().dump_live(q.dead_nodes()));
            needs_separator = true;
        }
        if args.types {
            if needs_separator {
                println!();
            }
            print!("{}", q.type_info().dump());
        }
        return;
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
        let src = query_source.as_ref().unwrap();
        eprint!("{}", q.diagnostics().render_colored(src, args.color));
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
