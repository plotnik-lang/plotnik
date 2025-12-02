mod output;
mod source;

use std::fs;
use std::io::{self, Read};

use plotnik_lib::Query;

use crate::cli::{OutputArgs, QueryArgs, SourceArgs};

pub fn run(
    query_args: QueryArgs,
    source_args: SourceArgs,
    lang: Option<String>,
    output: OutputArgs,
) {
    let has_query = query_args.query_text.is_some() || query_args.query_file.is_some();
    let has_source = source_args.source_text.is_some() || source_args.source_file.is_some();

    if let Err(msg) = validate_inputs(&output, has_query, has_source, &source_args, &lang) {
        eprintln!("error: {}", msg);
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
            && !output.source_ast_raw
            && !output.trace);

    let show_headers = count_outputs(&output, show_result) >= 2;

    // Load query if needed
    let query_source = if has_query {
        Some(load_query(&query_args))
    } else {
        None
    };
    let query = query_source.as_ref().map(|src| Query::new(src));

    if output.query_cst
        && let Some(ref q) = query {
            output::print_query_cst(q, show_headers);
        }

    if output.query_ast
        && let Some(ref q) = query {
            output::print_query_ast(q, show_headers);
        }

    if output.query_refs
        && let Some(ref q) = query
    {
        output::print_query_refs(q, show_headers);
    }

    if output.query_types {
        output::print_query_types(show_headers);
    }

    if output.source_ast {
        output::print_source_ast(&source_args, &lang, show_headers, false);
    }

    if output.source_ast_raw {
        output::print_source_ast(&source_args, &lang, show_headers, true);
    }

    if output.trace {
        output::print_trace(show_headers);
    }

    if show_result {
        output::print_result(show_headers);
    }

    if let Some(ref q) = query {
        output::print_query_errors(q);
    }
}

fn load_query(args: &QueryArgs) -> String {
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

fn validate_inputs(
    output: &OutputArgs,
    has_query: bool,
    has_source: bool,
    source_args: &SourceArgs,
    lang: &Option<String>,
) -> Result<(), &'static str> {
    if (output.query_cst || output.query_ast || output.query_refs || output.query_types)
        && !has_query
    {
        return Err(
            "--query-cst, --query-ast, --query-refs, and --query-types require --query-text or --query-file",
        );
    }

    if (output.source_ast || output.source_ast_raw) && !has_source {
        return Err("--source-ast and --source-ast-raw require --source-text or --source-file");
    }

    if output.trace && !(has_query && has_source) {
        return Err("--trace requires both query and source inputs");
    }

    if output.result && !(has_query && has_source) {
        return Err("--result requires both query and source inputs");
    }

    if source_args.source_text.is_some() && lang.is_none() {
        return Err("--lang is required when using --source-text");
    }

    Ok(())
}

fn count_outputs(output: &OutputArgs, show_result: bool) -> usize {
    [
        output.query_cst,
        output.query_ast,
        output.query_refs,
        output.query_types,
        output.source_ast,
        output.source_ast_raw,
        output.trace,
        show_result,
    ]
    .iter()
    .filter(|&&x| x)
    .count()
}
