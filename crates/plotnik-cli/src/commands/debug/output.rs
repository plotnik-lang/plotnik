use plotnik_lib::Query;

use super::source::{dump_source, load_source, parse_tree, resolve_lang};
use crate::cli::SourceArgs;

pub fn print_query_cst(query: &Query, show_header: bool) {
    if show_header {
        println!("=== QUERY CST ===");
    }
    print!("{}", query.dump_cst());
}

pub fn print_query_ast(query: &Query, show_header: bool) {
    if show_header {
        println!("=== QUERY AST ===");
    }
    print!("{}", query.dump_ast());
}

pub fn print_query_symbols(query: &Query, show_header: bool) {
    if show_header {
        println!("=== QUERY SYMBOLS ===");
    }
    print!("{}", query.dump_symbols());
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
    let output = dump_source(&tree, &source_code, include_anonymous);
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
        eprint!("{}", query.dump_errors_grouped());
    }
}
