//! Show AST of query and/or source file.

use std::path::PathBuf;

use arborium_tree_sitter as tree_sitter;
use plotnik_lib::QueryBuilder;

use super::query_loader::load_query_source;
use super::run_common;

pub struct AstArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub raw: bool,
    pub color: bool,
}

pub fn run(args: AstArgs) {
    let has_query = args.query_path.is_some() || args.query_text.is_some();
    let has_source = args.source_path.is_some() || args.source_text.is_some();

    if !has_query && !has_source {
        eprintln!("error: query or source required");
        std::process::exit(1);
    }

    let show_headers = has_query && has_source;

    // Show Query AST if query provided
    if has_query {
        if show_headers {
            println!("# Query AST");
        }
        print_query_ast(&args);
    }

    // Show Source AST if source provided
    if has_source {
        if show_headers {
            println!("\n# Source AST");
        }
        print_source_ast(&args);
    }
}

fn print_query_ast(args: &AstArgs) {
    let source_map = match load_query_source(args.query_path.as_deref(), args.query_text.as_deref())
    {
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

    let query = match QueryBuilder::new(source_map).parse() {
        Ok(parsed) => parsed.analyze(),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };

    // Show diagnostics if any (warnings)
    if query.diagnostics().has_errors() || query.diagnostics().has_warnings() {
        eprint!(
            "{}",
            query
                .diagnostics()
                .render_colored(query.source_map(), args.color)
        );
    }

    // Print AST (or CST if --raw)
    let output = query.printer().raw(args.raw).with_trivia(args.raw).dump();
    print!("{}", output);
}

fn print_source_ast(args: &AstArgs) {
    let source = run_common::load_source(
        args.source_text.as_deref(),
        args.source_path.as_deref(),
        args.query_path.as_deref(),
    );
    let lang = run_common::resolve_lang(args.lang.as_deref(), args.source_path.as_deref());
    let tree = lang.parse(&source);
    print!("{}", dump_tree(&tree, &source, args.raw));
}

fn dump_tree(tree: &tree_sitter::Tree, source: &str, raw: bool) -> String {
    format_node(tree.root_node(), source, 0, raw) + "\n"
}

fn format_node(
    node: tree_sitter::Node,
    source: &str,
    depth: usize,
    include_anonymous: bool,
) -> String {
    format_node_with_field(node, None, source, depth, include_anonymous)
}

fn format_node_with_field(
    node: tree_sitter::Node,
    field_name: Option<&str>,
    source: &str,
    depth: usize,
    include_anonymous: bool,
) -> String {
    if !include_anonymous && !node.is_named() {
        return String::new();
    }

    let indent = "  ".repeat(depth);
    let kind = node.kind();
    let field_prefix = field_name.map(|f| format!("{}: ", f)).unwrap_or_default();

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
        return if text == kind {
            format!("{}{}(\"{}\")", indent, field_prefix, escape_string(kind))
        } else {
            format!(
                "{}{}({} \"{}\")",
                indent,
                field_prefix,
                kind,
                escape_string(text)
            )
        };
    }

    let mut out = format!("{}{}({}", indent, field_prefix, kind);
    for (child, child_field) in children {
        out.push('\n');
        out.push_str(&format_node_with_field(
            child,
            child_field,
            source,
            depth + 1,
            include_anonymous,
        ));
    }
    out.push(')');
    out
}

fn escape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            c if c.is_control() => result.push_str(&format!("\\u{{{:04x}}}", c as u32)),
            c => result.push(c),
        }
    }
    result
}
