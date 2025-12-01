use crate::cli::{OutputArgs, QueryArgs, SourceArgs};
use crate::util::{load_query, load_source, parse_source_ast, resolve_lang};
use plotnik_lib::Query;

pub fn run(
    query_args: QueryArgs,
    source_args: SourceArgs,
    lang: Option<String>,
    output: OutputArgs,
) {
    let has_query = query_args.query_text.is_some() || query_args.query_file.is_some();
    let has_source = source_args.source_text.is_some() || source_args.source_file.is_some();

    // Validate output dependencies
    if (output.query_cst || output.query_ast || output.query_refs || output.query_types)
        && !has_query
    {
        eprintln!(
            "error: --query-cst, --query-ast, --query-refs, and --query-types require --query-text or --query-file"
        );
        std::process::exit(1);
    }

    if output.source_ast && !has_source {
        eprintln!("error: --source-ast requires --source-text or --source-file");
        std::process::exit(1);
    }

    if output.trace && !(has_query && has_source) {
        eprintln!("error: --trace requires both query and source inputs");
        std::process::exit(1);
    }

    if output.result && !(has_query && has_source) {
        eprintln!("error: --result requires both query and source inputs");
        std::process::exit(1);
    }

    // If both inputs provided and no outputs selected, default to --result
    let show_result = output.result
        || (has_query
            && has_source
            && !output.query_cst
            && !output.query_ast
            && !output.query_refs
            && !output.query_types
            && !output.source_ast
            && !output.trace);

    // Count selected outputs to decide whether to show section headers
    let output_count = [
        output.query_cst,
        output.query_ast,
        output.query_refs,
        output.query_types,
        output.source_ast,
        output.trace,
        show_result,
    ]
    .iter()
    .filter(|&&x| x)
    .count();
    let show_headers = output_count >= 2;

    // Validate --lang requirement
    if source_args.source_text.is_some() && lang.is_none() {
        eprintln!("error: --lang is required when using --source-text");
        std::process::exit(1);
    }

    // Load query if needed
    let query_source = if has_query {
        Some(load_query(&query_args))
    } else {
        None
    };

    let query = query_source.as_ref().map(|src| Query::new(src));

    if output.query_cst {
        if show_headers {
            println!("=== QUERY CST ===");
        }
        if let Some(ref q) = query {
            print!("{}", q.format_cst());
        }
    }

    if output.query_ast {
        if show_headers {
            println!("=== QUERY AST ===");
        }
        if let Some(ref q) = query {
            print!("{}", q.format_ast());
        }
    }

    if output.query_refs {
        if show_headers {
            println!("=== QUERY REFS ===");
        }
        if let Some(ref q) = query {
            print!("{}", q.format_refs());
        }
    }

    if output.query_types {
        if show_headers {
            println!("=== QUERY TYPES ===");
        }
        println!("(not implemented)");
        println!();
    }

    if output.source_ast {
        let resolved_lang = resolve_lang(&lang, &source_args);
        let source_code = load_source(&source_args);
        let sexp = parse_source_ast(&source_code, resolved_lang);
        if show_headers {
            println!("=== SOURCE AST ===");
        }
        println!("{}", sexp);
    }

    if output.trace {
        if show_headers {
            println!("=== TRACE ===");
        }
        println!("(not implemented)");
        println!();
    }

    if show_result {
        if show_headers {
            println!("=== RESULT ===");
        }
        println!("(not implemented)");
        println!();
    }

    // Print query errors at the end, grouped by stage
    if let Some(ref q) = query {
        if !q.is_valid() {
            eprint!("{}", q.render_errors_grouped());
        }
    }
}
