use std::fmt::Write;

use super::{GrammarNodeRef, Shape};

const INDENT: &str = "  ";

fn fits(col: usize, flat_len: usize, width: usize) -> bool {
    col + flat_len <= width
}

pub(super) fn render_shape(shape: &Shape, col: usize, indent: usize, width: usize) -> String {
    let flat = flat_shape(shape);
    if fits(col, flat.chars().count(), width) {
        return flat;
    }
    expand_shape(shape, col, indent, width)
}

pub(super) fn render_list(members: &[Shape], col: usize, indent: usize, width: usize) -> String {
    let flat = flat_group('[', ']', members);
    if fits(col, flat.chars().count(), width) {
        return flat;
    }
    expand_group('[', ']', members, indent, width)
}

fn flat_shape(shape: &Shape) -> String {
    match shape {
        Shape::Node(node) => node.to_string(),
        Shape::Token(token) => token.to_string(),
        Shape::Splice(name) => name.clone(),
        Shape::Empty => "{}".to_string(),
        Shape::Quantified(inner, quant) => format!("{}{}", flat_shape(inner), quant.marker()),
        Shape::Field(name, inner) => format!("{name}: {}", flat_shape(inner)),
        Shape::Seq(members) => flat_group('{', '}', members),
        Shape::Choice(members) => flat_group('[', ']', members),
    }
}

fn flat_group(open: char, close: char, members: &[Shape]) -> String {
    if members.is_empty() {
        return format!("{open}{close}");
    }
    let inner = members.iter().map(flat_shape).collect::<Vec<_>>().join(" ");
    format!("{open} {inner} {close}")
}

fn expand_shape(shape: &Shape, col: usize, indent: usize, width: usize) -> String {
    match shape {
        Shape::Seq(members) => expand_group('{', '}', members, indent, width),
        Shape::Choice(members) => expand_group('[', ']', members, indent, width),
        Shape::Quantified(inner, quant) => {
            format!(
                "{}{}",
                render_shape(inner, col, indent, width),
                quant.marker()
            )
        }
        Shape::Field(name, inner) => {
            let prefix = format!("{name}: ");
            let inner_col = col + prefix.chars().count();
            format!("{prefix}{}", render_shape(inner, inner_col, indent, width))
        }
        _ => flat_shape(shape),
    }
}

fn expand_group(open: char, close: char, members: &[Shape], indent: usize, width: usize) -> String {
    if members.is_empty() {
        return format!("{open}{close}");
    }
    let inner_indent = indent + 1;
    let pad = INDENT.repeat(inner_indent);
    let member_col = inner_indent * INDENT.len();
    let mut out = String::new();
    out.push(open);
    for member in members {
        let rendered = render_shape(member, member_col, inner_indent, width);
        let _ = write!(out, "\n{pad}{rendered}");
    }
    let _ = write!(out, "\n{}{close}", INDENT.repeat(indent));
    out
}

pub(super) fn flat_category(members: &[GrammarNodeRef]) -> String {
    members
        .iter()
        .map(category_member)
        .collect::<Vec<_>>()
        .join(" | ")
}

pub(super) fn expand_category(members: &[GrammarNodeRef]) -> String {
    let mut out = String::new();
    for member in members {
        let _ = write!(out, "\n{INDENT}| {}", category_member(member));
    }
    out
}

fn category_member(node_ref: &GrammarNodeRef) -> String {
    if node_ref.category {
        format!("{}#", node_ref.name)
    } else {
        node_ref.name.clone()
    }
}
