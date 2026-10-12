//! Identifier casing and Rust keyword hygiene shared by type and code emitters.

use std::collections::HashSet;

/// PascalCase → SHOUTY_SNAKE (`FooBar` → `FOO_BAR`, `HTTPServer` →
/// `HTTP_SERVER`).
pub(crate) fn shouty_ident(name: &str) -> String {
    case_segments(name)
        .map(|segment| segment.to_uppercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// PascalCase → snake_case (`FooBar` → `foo_bar`).
pub(crate) fn snake_ident(name: &str) -> String {
    case_segments(name)
        .map(|segment| segment.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// Keywords with no `r#` form.
const RUST_UNRAW: &[&str] = &["crate", "self", "super", "Self"];

/// Reserved words through Rust 2024 that are valid as raw identifiers.
const RUST_RAW: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "do", "dyn",
    "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl", "in", "let",
    "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref", "return",
    "static", "struct", "trait", "true", "try", "type", "typeof", "unsafe", "use", "virtual",
    "where", "while", "yield",
];

/// Hygienic Rust identifiers for one scope, in input order. Underscore-renamed
/// keywords keep growing underscores until unique, so `self` and `self_` can
/// coexist without changing their query-side serialized names.
pub(crate) fn rust_scope_idents<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut taken = HashSet::new();
    names
        .map(|name| {
            let mut ident = rust_ident(name);
            while !taken.insert(ident.clone()) {
                ident.push('_');
            }
            ident
        })
        .collect()
}

fn rust_ident(name: &str) -> String {
    if RUST_UNRAW.contains(&name) {
        format!("{name}_")
    } else if RUST_RAW.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

/// Split camel/Pascal humps: a boundary opens before an uppercase following a
/// non-uppercase, and before the last uppercase of an acronym run.
fn case_segments(name: &str) -> impl Iterator<Item = String> {
    let chars: Vec<char> = name.chars().collect();
    let mut segments = Vec::new();
    let mut current = String::new();
    for (index, &ch) in chars.iter().enumerate() {
        if ch == '_' {
            if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }
            continue;
        }
        let previous_is_upper = index > 0 && chars[index - 1].is_uppercase();
        let next_is_lower = chars.get(index + 1).is_some_and(|next| next.is_lowercase());
        let boundary =
            ch.is_uppercase() && !current.is_empty() && (!previous_is_upper || next_is_lower);
        if boundary {
            segments.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments.into_iter()
}
