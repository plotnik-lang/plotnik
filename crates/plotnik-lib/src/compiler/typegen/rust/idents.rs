//! Rust identifier hygiene for query-supplied names.
//!
//! Capture names, definition names, and enum labels come from the query with
//! Plotnik's own rules (snake_case / PascalCase), which still admit Rust
//! keywords — `@type`, `@match`, a `Self` label. Keywords render as raw
//! identifiers; the four that have no raw form get a trailing underscore, with
//! per-scope disambiguation in case the underscored spelling is itself taken.
//! Renaming affects only the Rust spelling: serialized field names and enum
//! tags always use the original query-side string.

use std::collections::HashSet;

/// Keywords with no `r#` form.
const UNRAW: &[&str] = &["crate", "self", "super", "Self"];

/// Reserved words (through edition 2024) that are valid as raw identifiers.
const RAW: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "do", "dyn",
    "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl", "in", "let",
    "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref", "return",
    "static", "struct", "trait", "true", "try", "type", "typeof", "unsafe", "use", "virtual",
    "where", "while", "yield",
];

fn rust_ident(name: &str) -> String {
    if UNRAW.contains(&name) {
        format!("{name}_")
    } else if RAW.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

/// Hygienic identifiers for one scope (a struct's fields, an enum's variants,
/// the module's items), in input order. Underscore-renamed keywords keep
/// growing underscores until unique, so a scope holding both `self` and
/// `self_` still renders collision-free.
pub(crate) fn scope_idents<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut taken: HashSet<String> = HashSet::new();
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
