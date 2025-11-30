use crate::ql::parser::{parse, render_errors};
use crate::ql::syntax_kind::SyntaxNode;

/// Format tree without trivia tokens (default for most tests)
pub fn snapshot(input: &str) -> String {
    format_result(input, false)
}

/// Format tree with trivia tokens included
pub fn snapshot_raw(input: &str) -> String {
    format_result(input, true)
}

pub fn format_result(input: &str, include_trivia: bool) -> String {
    let result = parse(input);
    let mut out = String::new();
    format_tree_impl(&result.syntax(), 0, &mut out, include_trivia);
    if !result.errors().is_empty() {
        out.push_str("---\n");
        out.push_str(&render_errors(input, result.errors()));
        out.push('\n');
    }
    out
}

fn format_tree_impl(node: &SyntaxNode, indent: usize, out: &mut String, include_trivia: bool) {
    use std::fmt::Write;
    let prefix = "  ".repeat(indent);
    let _ = writeln!(out, "{}{:?}", prefix, node.kind());
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => format_tree_impl(&n, indent + 1, out, include_trivia),
            rowan::NodeOrToken::Token(t) => {
                if include_trivia || !t.kind().is_trivia() {
                    let _ = writeln!(out, "{}  {:?} {:?}", prefix, t.kind(), t.text());
                }
            }
        }
    }
}
