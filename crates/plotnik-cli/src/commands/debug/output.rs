use super::source::{format_ast, load_source, parse_tree, resolve_lang};
use crate::cli::SourceArgs;
use plotnik_lib::Query;

pub fn print_query_cst(query: &Query, show_header: bool) {
    if show_header {
        println!("=== QUERY CST ===");
    }
    print!("{}", query.format_cst());
}

pub fn print_query_ast(query: &Query, show_header: bool) {
    if show_header {
        println!("=== QUERY AST ===");
    }
    print!("{}", query.format_ast());
}

pub fn print_query_refs(query: &Query, show_header: bool) {
    if show_header {
        println!("=== QUERY REFS ===");
    }
    print!("{}", query.format_refs());
}

pub fn print_query_types(show_header: bool) {
    if show_header {
        println!("=== QUERY TYPES ===");
    }
    println!("(not implemented)");
    println!();
}

pub fn print_source_ast(
    source_args: &SourceArgs,
    lang: &Option<String>,
    show_header: bool,
    include_anonymous: bool,
) {
    let resolved_lang = resolve_lang(lang, source_args);
    let source_code = load_source(source_args);
    let tree = parse_tree(&source_code, resolved_lang);
    let output = format_ast(&tree, &source_code, include_anonymous);
    if show_header {
        if include_anonymous {
            println!("=== SOURCE AST RAW ===");
        } else {
            println!("=== SOURCE AST ===");
        }
    }
    print!("{}", output);
}

pub fn print_trace(show_header: bool) {
    if show_header {
        println!("=== TRACE ===");
    }
    println!("(not implemented)");
    println!();
}

pub fn print_result(show_header: bool) {
    if show_header {
        println!("=== RESULT ===");
    }
    println!("(not implemented)");
    println!();
}

pub fn print_query_errors(query: &Query) {
    if !query.is_valid() {
        eprint!("{}", query.render_errors_grouped());
    }
}