use std::fs;
use std::io::{self, Read};

use plotnik_langs::Lang;

use crate::cli::SourceArgs;

pub fn load_source(args: &SourceArgs) -> String {
    if let Some(text) = &args.source_text {
        return text.clone();
    }
    if let Some(path) = &args.source_file {
        if path.as_os_str() == "-" {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .expect("failed to read stdin");
            return buf;
        }
        return fs::read_to_string(path).expect("failed to read source file");
    }
    unreachable!()
}

pub fn resolve_lang(lang: &Option<String>, source_args: &SourceArgs) -> Lang {
    if let Some(name) = lang {
        return Lang::from_name(name).unwrap_or_else(|| {
            eprintln!("error: unknown language: {}", name);
            std::process::exit(1);
        });
    }

    if let Some(path) = &source_args.source_file
        && path.as_os_str() != "-"
        && let Some(ext) = path.extension().and_then(|e| e.to_str())
    {
        return Lang::from_extension(ext).unwrap_or_else(|| {
            eprintln!(
                "error: cannot infer language from extension '.{}', use --lang",
                ext
            );
            std::process::exit(1);
        });
    }

    eprintln!("error: --lang is required (cannot infer from input)");
    std::process::exit(1);
}

pub fn parse_tree(source: &str, lang: Lang) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang.language())
        .expect("failed to set language");
    parser.parse(source, None).expect("failed to parse source")
}

pub fn format_ast(tree: &tree_sitter::Tree, source: &str, include_anonymous: bool) -> String {
    format_node(tree.root_node(), source, 0, include_anonymous) + "\n"
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
        if text == kind {
            format!("{}{}(\"{}\")", indent, field_prefix, escape_string(kind))
        } else {
            format!(
                "{}{}({} \"{}\")",
                indent,
                field_prefix,
                kind,
                escape_string(text)
            )
        }
    } else {
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
