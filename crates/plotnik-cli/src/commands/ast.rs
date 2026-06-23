//! Show AST of query and/or source file.

use std::fmt::Write as _;
use std::path::PathBuf;

use arborium_tree_sitter as tree_sitter;
use plotnik_lib::QueryBuilder;

use super::lang_resolver::reconcile_lang;
use super::query_loader::load_query;
use super::run_common;
use crate::error::{CliError, CliResult};

pub struct AstArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub raw: bool,
    pub color: bool,
}

pub fn run(args: AstArgs) -> CliResult {
    let has_query = args.query_path.is_some() || args.query_text.is_some();
    let has_source = args.source_path.is_some() || args.source_text.is_some();

    if !has_query && !has_source {
        return Err(CliError::fatal("query or source required"));
    }

    let show_headers = has_query && has_source;

    let mut declared_lang = None;
    if has_query {
        if show_headers {
            println!("# Query AST");
        }
        declared_lang = print_query_ast(&args)?;
    }

    if has_source {
        if show_headers {
            println!("\n# Source AST");
        }
        print_source_ast(&args, declared_lang.as_deref())?;
    }

    Ok(())
}

/// Prints the query AST; returns the shebang-declared language, if any.
fn print_query_ast(args: &AstArgs) -> Result<Option<String>, CliError> {
    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    // Enforce -l/shebang agreement even when no source AST follows
    reconcile_lang(args.lang.as_deref(), loaded.shebang.lang.as_deref())?;

    let query = QueryBuilder::new(loaded.sources)
        .analyze()
        .map_err(|e| CliError::fatal(e.to_string()))?;

    if query.diagnostics().has_errors() || query.diagnostics().has_warnings() {
        eprint!(
            "{}",
            query
                .diagnostics()
                .render_colored(query.source_map(), args.color)
        );
    }

    let output = if args.raw {
        query.dump_cst_with_trivia(true)
    } else {
        query.dump_ast()
    };
    print!("{}", output);

    Ok(loaded.shebang.lang)
}

fn print_source_ast(args: &AstArgs, declared_lang: Option<&str>) -> CliResult {
    let source = run_common::load_source(
        args.source_text.as_deref(),
        args.source_path.as_deref(),
        args.query_path.as_deref(),
    )?;
    let lang = run_common::resolve_run_lang(
        args.lang.as_deref(),
        declared_lang,
        args.source_path.as_deref(),
    )?;
    let tree = lang.parse_source(&source);
    print!("{}", dump_tree(&tree, &source, args.raw));

    Ok(())
}

fn dump_tree(tree: &tree_sitter::Tree, source: &str, raw: bool) -> String {
    let mut out = String::new();
    format_node(&mut out, tree.root_node(), source, 0, raw);
    out.push('\n');
    out
}

fn format_node(
    out: &mut String,
    node: tree_sitter::Node,
    source: &str,
    depth: usize,
    include_anonymous: bool,
) {
    format_node_with_field(out, node, None, source, depth, include_anonymous);
}

fn format_node_with_field(
    out: &mut String,
    node: tree_sitter::Node,
    field_name: Option<&str>,
    source: &str,
    depth: usize,
    include_anonymous: bool,
) {
    if !include_anonymous && !node.is_named() {
        return;
    }

    for _ in 0..depth {
        out.push_str("  ");
    }
    if let Some(f) = field_name {
        let _ = write!(out, "{}: ", f);
    }
    let kind = node.kind();

    let children: Vec<_> = {
        let mut cursor = node.walk();
        let mut result = Vec::new();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if include_anonymous || child.is_named() {
                    result.push((child, cursor.field_name()));
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        result
    };

    if children.is_empty() {
        let text = node
            .utf8_text(source.as_bytes())
            .unwrap_or("<invalid utf8>");
        if text == kind {
            out.push_str("(\"");
            escape_string_into(out, kind);
            out.push_str("\")");
        } else {
            let _ = write!(out, "({} \"", kind);
            escape_string_into(out, text);
            out.push_str("\")");
        }
        return;
    }

    let _ = write!(out, "({}", kind);
    for (child, child_field) in children {
        out.push('\n');
        format_node_with_field(
            out,
            child,
            child_field,
            source,
            depth + 1,
            include_anonymous,
        );
    }
    out.push(')');
}

fn escape_string_into(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:04x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
}
