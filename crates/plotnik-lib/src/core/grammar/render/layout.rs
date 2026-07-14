//! Width-aware layout of shapes and categories.
//!
//! A composite group renders inline (`{ a b c }`) when its single-line form fits
//! within the baseline width at the column it starts on, and breaks one member
//! per line otherwise. The decision is funnelled through [`fits`] — the single
//! width knob — so the policy lives in one place: `width = 0` forces every group
//! to break (the old always-break behaviour), a very large width keeps everything
//! inline. Quantifiers and fields never separate from their operand.

use std::fmt::Write;

use super::{GrammarNodeRef, Shape};

const INDENT: &str = "  ";

/// Whether `flat_len` characters starting at column `col` fit within `width`.
/// The one place the width policy is decided.
fn fits(col: usize, flat_len: usize, width: usize) -> bool {
    col + flat_len <= width
}

/// Render a shape as a pattern body. `col` is the column its first character
/// lands on; `indent` is the level its continuation and closing lines align to.
pub(super) fn render_shape(shape: &Shape, col: usize, indent: usize, width: usize) -> String {
    let flat = flat_shape(shape);
    if fits(col, flat.chars().count(), width) {
        return flat;
    }
    expand_shape(shape, col, indent, width)
}

/// Render a borrowed slice of shapes as a bracketed `[...]` group (the extras
/// list), folding inline when it fits.
pub(super) fn render_list(members: &[Shape], col: usize, indent: usize, width: usize) -> String {
    let flat = flat_group('[', ']', members);
    if fits(col, flat.chars().count(), width) {
        return flat;
    }
    expand_group('[', ']', members, indent, width)
}

/// The single-line form of a shape, with no width checks.
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

/// Render a shape across multiple lines — reached only when its flat form did
/// not fit. Members re-decide their own fit at the deeper indent, so a short
/// member stays inline inside a broken parent.
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
        // Atoms have no multi-line form.
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

/// The single-line form of a category's members: `a | b | c#`.
pub(super) fn flat_category(members: &[GrammarNodeRef]) -> String {
    members
        .iter()
        .map(category_member)
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The broken form of a category: one member per line with a leading `|`.
pub(super) fn expand_category(members: &[GrammarNodeRef]) -> String {
    let mut out = String::new();
    for member in members {
        let _ = write!(out, "\n{INDENT}| {}", category_member(member));
    }
    out
}

/// A category member is written bare (no parens); a nested category keeps its `#`.
fn category_member(node_ref: &GrammarNodeRef) -> String {
    if node_ref.category {
        format!("{}#", node_ref.name)
    } else {
        node_ref.name.clone()
    }
}
