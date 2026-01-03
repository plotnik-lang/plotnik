use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use arborium_tree_sitter as tree_sitter;
use plotnik_langs::Lang;

pub struct TreeArgs {
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub raw: bool,
    pub spans: bool,
}

pub fn run(args: TreeArgs) {
    let source = match (&args.source_text, &args.source_path) {
        (Some(text), None) => text.clone(),
        (None, Some(path)) => load_source(path),
        (Some(_), Some(_)) => {
            eprintln!("error: cannot use both --source and positional SOURCE");
            std::process::exit(1);
        }
        (None, None) => {
            eprintln!("error: source required (positional or --source)");
            std::process::exit(1);
        }
    };

    let lang = resolve_lang(&args.lang, args.source_path.as_deref(), args.source_text.is_some());
    let tree = lang.parse(&source);
    print!("{}", dump_tree(&tree, &source, args.raw, args.spans));
}

fn load_source(path: &PathBuf) -> String {
    if path.as_os_str() == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .expect("failed to read stdin");
        return buf;
    }
    fs::read_to_string(path).unwrap_or_else(|_| {
        eprintln!("error: file not found: {}", path.display());
        std::process::exit(1);
    })
}

fn resolve_lang(lang: &Option<String>, source_path: Option<&Path>, is_inline: bool) -> Lang {
    if let Some(name) = lang {
        return plotnik_langs::from_name(name).unwrap_or_else(|| {
            eprintln!("error: unknown language: {}", name);
            std::process::exit(1);
        });
    }

    if let Some(path) = source_path
        && path.as_os_str() != "-"
        && let Some(ext) = path.extension().and_then(|e| e.to_str())
    {
        return plotnik_langs::from_ext(ext).unwrap_or_else(|| {
            eprintln!(
                "error: cannot infer language from extension '.{}', use -l/--lang",
                ext
            );
            std::process::exit(1);
        });
    }

    if is_inline {
        eprintln!("error: -l/--lang is required when using inline source");
    } else {
        eprintln!("error: -l/--lang is required (cannot infer from stdin)");
    }
    std::process::exit(1);
}

fn dump_tree(tree: &tree_sitter::Tree, source: &str, raw: bool, spans: bool) -> String {
    format_node(tree.root_node(), source, 0, raw, spans) + "\n"
}

fn format_node(
    node: tree_sitter::Node,
    source: &str,
    depth: usize,
    include_anonymous: bool,
    show_spans: bool,
) -> String {
    format_node_with_field(node, None, source, depth, include_anonymous, show_spans)
}

fn format_node_with_field(
    node: tree_sitter::Node,
    field_name: Option<&str>,
    source: &str,
    depth: usize,
    include_anonymous: bool,
    show_spans: bool,
) -> String {
    if !include_anonymous && !node.is_named() {
        return String::new();
    }

    let indent = "  ".repeat(depth);
    let kind = node.kind();
    let field_prefix = field_name.map(|f| format!("{}: ", f)).unwrap_or_default();
    let span_suffix = if show_spans {
        let start = node.start_position();
        let end = node.end_position();
        format!(" [{}:{}-{}:{}]", start.row, start.column, end.row, end.column)
    } else {
        String::new()
    };

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
            format!(
                "{}{}(\"{}\"){}",
                indent,
                field_prefix,
                escape_string(kind),
                span_suffix
            )
        } else {
            format!(
                "{}{}({} \"{}\"){}",
                indent,
                field_prefix,
                kind,
                escape_string(text),
                span_suffix
            )
        };
    }

    let mut out = format!("{}{}({}{}", indent, field_prefix, kind, span_suffix);
    for (child, child_field) in children {
        out.push('\n');
        out.push_str(&format_node_with_field(
            child,
            child_field,
            source,
            depth + 1,
            include_anonymous,
            show_spans,
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
